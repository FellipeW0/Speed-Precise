//! Live DirectInput8 calibration read/write (`DIPROP_CALIBRATION = 11 / 0x0B`),
//! deadzone (`DIPROP_DEADZONE = 5`), saturation (`DIPROP_SATURATION = 6`),
//! DIView normalization math, and automatic `%APPDATA%\SpeedPrecise\backups\` persistence.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::dinput_ffi::{
    makediprop, DipropCal, DipropDword, DipropRange, IDirectInputDevice8W,
    DIPH_BYID, DIPROPCALIBRATIONMODE_COOKED, DIPROPCALIBRATIONMODE_RAW, DIPROP_CALIBRATION,
    DIPROP_CALIBRATIONMODE, DIPROP_DEADZONE, DIPROP_RANGE, DIPROP_SATURATION,
};
use crate::error::{check_hresult, CoreError, CoreResult};

/// Represents the 3-point DirectInput8 calibration tuple `[lMin, lCenter, lMax]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisCalibration {
    pub min: i32,
    pub center: i32,
    pub max: i32,
}

impl AxisCalibration {
    /// Creates and validates a 3-point calibration tuple (`min <= center <= max` and `min < max`).
    pub fn new(min: i32, center: i32, max: i32) -> CoreResult<Self> {
        if min >= max || center < min || center > max {
            return Err(CoreError::InvalidCalibrationBounds { min, center, max });
        }
        Ok(Self { min, center, max })
    }

    /// Creates an unchecked `AxisCalibration` (used when reading existing raw driver properties
    /// that may hold uninitialized or inverted factory defaults).
    #[inline]
    pub const fn new_unchecked(min: i32, center: i32, max: i32) -> Self {
        Self { min, center, max }
    }

    /// Computes DIView's default reset calibration from `[logical_min, logical_max]`.
    ///
    /// Verified in `DIView.exe` `FUN_00401f20` (line 1538):
    /// `lCenter = lMin + ((lMax - lMin + 1) / 2)`
    #[inline]
    pub const fn default_reset(logical_min: i32, logical_max: i32) -> Self {
        let span = (logical_max as i64) - (logical_min as i64) + 1;
        let center = (logical_min as i64) + (span / 2);
        Self {
            min: logical_min,
            center: center as i32,
            max: logical_max,
        }
    }

    /// Converts this calibration into a 28-byte `DipropCal` struct (`DIPH_BYID = 2`).
    #[inline]
    pub const fn to_dipropcal(self, axis_dw_type: u32) -> DipropCal {
        DipropCal::new(axis_dw_type, DIPH_BYID, self.min, self.center, self.max)
    }

    /// Serializes `[min, center, max]` into the 12-byte Little-Endian `REG_BINARY` payload
    /// stored under `HKCU\...\DirectInput\VID_%04X&PID_%04X\Calibration\0\Type\Axes\%d`.
    #[inline]
    pub const fn to_registry_bytes(self) -> [u8; 12] {
        self.to_dipropcal(0).to_registry_bytes()
    }

    /// Deserializes a 12-byte Little-Endian `REG_BINARY` calibration payload.
    #[inline]
    pub fn from_registry_bytes(bytes: &[u8; 12]) -> Self {
        let min = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let center = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let max = i32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        Self { min, center, max }
    }

    /// Replicates DIView's exact raw-value normalization formula (`FUN_00402210`, lines 1665-1677)
    /// with epsilon `0.0001` / `0.0002` to prevent division-by-zero when `min == max`.
    #[inline]
    pub fn normalize_raw_diview(self, raw_val: i32) -> f64 {
        let num = (raw_val as f64) - (self.min as f64) + 0.0001;
        let den = (self.max as f64) - (self.min as f64) + 0.0002;
        (num / den).clamp(0.0, 1.0)
    }
}

/// Computes smart `[lMin, lCenter, lMax]` DirectInput8 pedal calibration (`DIPROP_CALIBRATION = 0x0B`)
/// from consumer-friendly **Max Travel / Saturation (`10%..=100%`)** and **Initial Deadzone (`0%..=30%`)**
/// sliders, replacing DirectInput's center-symmetric `DIPROP_SATURATION` property on unilateral pedals.
///
/// ### Inverted Pedals (`inverted == true`, e.g. Logitech G29/G920 `0..=255` where resting = `lMax` and full press = `lMin`)
/// - `span = raw_rest - raw_full = logical_max - logical_min`
/// - `lMax = round(raw_rest - (deadzone_pct / 100.0) * span)`
/// - `lMin = round(raw_rest - (saturation_pct / 100.0) * span)`
/// - `lCenter = floor((lMin + lMax) / 2)`
///
/// ### Non-Inverted Pedals (`inverted == false`, where resting = `logical_min` and full press = `logical_max`)
/// - `span = raw_full - raw_rest = logical_max - logical_min`
/// - `lMin = round(raw_rest + (deadzone_pct / 100.0) * span)`
/// - `lMax = round(raw_rest + (saturation_pct / 100.0) * span)`
/// - `lCenter = floor((lMin + lMax) / 2)`
pub fn compute_smart_pedal_calibration(
    logical_min: i32,
    logical_max: i32,
    saturation_pct: f64,
    deadzone_pct: f64,
    inverted: bool,
) -> CoreResult<AxisCalibration> {
    if logical_min >= logical_max {
        return Err(CoreError::InvalidCalibrationBounds {
            min: logical_min,
            center: logical_min,
            max: logical_max,
        });
    }

    let dz = deadzone_pct.clamp(0.0, 30.0);
    let sat = saturation_pct.clamp(10.0, 100.0).max(dz + 1.0).min(100.0);
    let span = (logical_max as f64) - (logical_min as f64);

    let (mut l_min, mut l_max) = if inverted {
        let raw_rest = logical_max as f64;
        let max_val = (raw_rest - (dz / 100.0) * span).round() as i32;
        let min_val = (raw_rest - (sat / 100.0) * span).round() as i32;
        (min_val, max_val)
    } else {
        let raw_rest = logical_min as f64;
        let min_val = (raw_rest + (dz / 100.0) * span).round() as i32;
        let max_val = (raw_rest + (sat / 100.0) * span).round() as i32;
        (min_val, max_val)
    };

    l_min = l_min.clamp(logical_min, logical_max);
    l_max = l_max.clamp(logical_min, logical_max);

    if l_min >= l_max {
        if l_max > logical_min {
            l_min = l_max - 1;
        } else {
            l_max = l_min + 1;
        }
    }

    let l_center = ((l_min as i64) + (l_max as i64)).div_euclid(2) as i32;
    AxisCalibration::new(l_min, l_center, l_max)
}

/// Infers `(saturation_pct, deadzone_pct)` in `10.0..=100.0` and `0.0..=30.0` from an existing
/// `[lMin, lCenter, lMax]` Registry calibration so UI sliders initialize accurately to the user's
/// active pedal setup without overwriting custom values on startup.
pub fn infer_pedal_sliders_from_calibration(
    logical_min: i32,
    logical_max: i32,
    cal: AxisCalibration,
    inverted: bool,
) -> (f64, f64) {
    if logical_min >= logical_max || cal.min >= cal.max {
        return (100.0, 0.0);
    }

    let span = (logical_max as f64) - (logical_min as f64);
    let (raw_sat, raw_dz) = if inverted {
        let rest = logical_max as f64;
        let dz = ((rest - (cal.max as f64)) / span) * 100.0;
        let sat = ((rest - (cal.min as f64)) / span) * 100.0;
        (sat, dz)
    } else {
        let rest = logical_min as f64;
        let dz = (((cal.min as f64) - rest) / span) * 100.0;
        let sat = (((cal.max as f64) - rest) / span) * 100.0;
        (sat, dz)
    };

    let dz_clamped = ((raw_dz * 10.0).round() / 10.0).clamp(0.0, 30.0);
    let sat_clamped = ((raw_sat * 10.0).round() / 10.0)
        .clamp(10.0, 100.0)
        .max(dz_clamped + 1.0)
        .min(100.0);
    (sat_clamped, dz_clamped)
}

/// Evaluates DirectInput8's exact 3-point `[lMin, lCenter, lMax]` piecewise-linear calibration
/// transfer curve (plus `deadzone` and `saturation` in `0..=10000`) for a given `raw_value`,
/// returning the calibrated `cooked_percent` in `0.0..=100.0`.
///
/// This mirrors `dinput8.dll`'s internal axis ramp (`CJoy_CookAxis`) while remaining immune to
/// `dinput8.dll`'s post-`SetProperty` state-buffer reset to `32767` (`50.0%`) when no new USB HID
/// interrupt report has been emitted yet for a stationary pedal.
#[inline]
pub fn evaluate_calibrated_output_pct(
    raw_value: i32,
    cal: AxisCalibration,
    deadzone: u32,
    saturation: u32,
) -> f64 {
    if cal.min >= cal.max {
        return 50.0;
    }

    let dz_frac = (deadzone.min(10_000) as f64) / 10_000.0;
    let sat_frac = ((saturation.min(10_000) as f64) / 10_000.0)
        .max(dz_frac + 1e-6)
        .min(1.0);

    if raw_value <= cal.center {
        let low_span = (cal.center as f64) - (cal.min as f64);
        if low_span <= f64::EPSILON {
            return if raw_value < cal.min { 0.0 } else { 50.0 };
        }
        let dist = (((cal.center as f64) - (raw_value as f64)) / low_span).max(0.0);
        let norm_below = if dist <= dz_frac {
            0.0
        } else if dist >= sat_frac {
            1.0
        } else {
            (dist - dz_frac) / (sat_frac - dz_frac)
        };
        (50.0 * (1.0 - norm_below)).clamp(0.0, 50.0)
    } else {
        let high_span = (cal.max as f64) - (cal.center as f64);
        if high_span <= f64::EPSILON {
            return 100.0;
        }
        let dist = (((raw_value as f64) - (cal.center as f64)) / high_span).max(0.0);
        let norm_above = if dist <= dz_frac {
            0.0
        } else if dist >= sat_frac {
            1.0
        } else {
            (dist - dz_frac) / (sat_frac - dz_frac)
        };
        (50.0 + 50.0 * norm_above).clamp(50.0, 100.0)
    }
}

/// Converts raw hardware encoder reading + DirectInput `cooked_percent` into cockpit-oriented
/// `(physical_travel_pct, ingame_output_pct)` in `0.0..=100.0`, where `0.0%` is always pedal
/// released (bottom of vertical bar) and `100.0%` is always pedal fully pressed (top of vertical bar).
#[inline]
pub fn compute_pedal_press_percentages(
    raw_value: i32,
    logical_range: (i32, i32),
    cooked_percent: f64,
    inverted: bool,
) -> (f64, f64) {
    let (l_min, l_max) = logical_range;
    let span = ((l_max as f64) - (l_min as f64)).abs().max(1.0);
    let cooked_clamped = cooked_percent.clamp(0.0, 100.0);

    if inverted {
        let physical = (((l_max as f64) - (raw_value as f64)) / span * 100.0).clamp(0.0, 100.0);
        let ingame = (100.0 - cooked_clamped).clamp(0.0, 100.0);
        (physical, ingame)
    } else {
        let physical = (((raw_value as f64) - (l_min as f64)) / span * 100.0).clamp(0.0, 100.0);
        (physical, cooked_clamped)
    }
}

/// Converts calibrated steering `cooked_percent` (`0.0..=100.0`, where `50.0%` is center `0°`)
/// into `(angle_deg, signed_percent)` across `[-wheel_rotation_deg / 2, +wheel_rotation_deg / 2]`
/// and `[-100.0%, +100.0%]`.
#[inline]
pub fn compute_steering_angle_deg(cooked_percent: f64, wheel_rotation_deg: f64) -> (f64, f64) {
    let clamped_pct = cooked_percent.clamp(0.0, 100.0);
    let signed_pct = ((clamped_pct - 50.0) * 2.0).clamp(-100.0, 100.0);
    let half_lock = wheel_rotation_deg.clamp(90.0, 3_600.0) * 0.5;
    let angle_deg = (signed_pct / 100.0) * half_lock;
    (angle_deg, signed_pct)
}

/// JSON backup record persisted to `%APPDATA%\SpeedPrecise\backups\` before writing live calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationBackupRecord {
    pub timestamp_unix_secs: u64,
    pub vid_hex: String,
    pub pid_hex: String,
    pub device_name: String,
    pub axis_dw_type: u32,
    pub previous_calibration: AxisCalibration,
    pub previous_deadzone: Option<u32>,
    pub previous_saturation: Option<u32>,
    pub registry_binary_hex: String,
}

/// Resolves the backup directory (`%APPDATA%\SpeedPrecise\backups\`).
pub fn default_backup_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("SpeedPrecise").join("backups")
    } else {
        std::env::temp_dir().join("SpeedPrecise").join("backups")
    }
}

/// Persists a calibration backup file into `backup_dir` before mutating a device's calibration.
pub fn persist_calibration_backup_in_dir(
    backup_dir: &Path,
    vid: u16,
    pid: u16,
    device_name: &str,
    axis_dw_type: u32,
    prev_cal: AxisCalibration,
    prev_deadzone: Option<u32>,
    prev_saturation: Option<u32>,
) -> CoreResult<PathBuf> {
    fs::create_dir_all(backup_dir).map_err(|source| CoreError::BackupIo {
        path: backup_dir.display().to_string(),
        source,
    })?;

    let timestamp_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let reg_bytes = prev_cal.to_registry_bytes();
    let registry_binary_hex = reg_bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");

    let record = CalibrationBackupRecord {
        timestamp_unix_secs,
        vid_hex: format!("0x{vid:04X}"),
        pid_hex: format!("0x{pid:04X}"),
        device_name: device_name.to_string(),
        axis_dw_type,
        previous_calibration: prev_cal,
        previous_deadzone: prev_deadzone,
        previous_saturation: prev_saturation,
        registry_binary_hex,
    };

    let filename = format!("VID_{vid:04X}_PID_{pid:04X}_axis_0x{axis_dw_type:08X}_backup.json");
    let file_path = backup_dir.join(filename);
    let json = serde_json::to_string_pretty(&record)?;
    fs::write(&file_path, json).map_err(|source| CoreError::BackupIo {
        path: file_path.display().to_string(),
        source,
    })?;

    Ok(file_path)
}

/// Reads an axis range (`DIPROP_RANGE = 4`, `DIPROP_PHYSICALRANGE = 18`, or `DIPROP_LOGICALRANGE = 19`)
/// using `DIPH_BYID = 2`.
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn get_axis_range(
    device: *mut IDirectInputDevice8W,
    prop_id: usize,
    axis_dw_type: u32,
) -> CoreResult<(i32, i32)> {
    let mut range = DipropRange::new(axis_dw_type, DIPH_BYID, 0, 0);
    let hr = ((*(*device).lp_vtbl).get_property)(
        device,
        makediprop(prop_id),
        &mut range.diph,
    );
    check_hresult("IDirectInputDevice8::GetProperty(Range)", hr)?;
    Ok((range.l_min, range.l_max))
}

/// Sets an axis output range (`DIPROP_RANGE = 4`) using `DIPH_BYID = 2`.
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn set_axis_range(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
    min: i32,
    max: i32,
) -> CoreResult<()> {
    let range = DipropRange::new(axis_dw_type, DIPH_BYID, min, max);
    let hr = ((*(*device).lp_vtbl).set_property)(
        device,
        makediprop(DIPROP_RANGE),
        &range.diph,
    );
    check_hresult("IDirectInputDevice8::SetProperty(DIPROP_RANGE)", hr)
}

/// Sets `DIPROP_CALIBRATIONMODE` (`0x0A`) on a specific axis (`DIPH_BYID = 2`).
/// When `raw_mode == true`, sets `DIPROPCALIBRATIONMODE_RAW (1)`, bypassing calibration/deadzone/saturation.
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn set_calibration_mode(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
    raw_mode: bool,
) -> CoreResult<()> {
    let mode = if raw_mode {
        DIPROPCALIBRATIONMODE_RAW
    } else {
        DIPROPCALIBRATIONMODE_COOKED
    };
    let prop = DipropDword::new(axis_dw_type, DIPH_BYID, mode);
    let hr = ((*(*device).lp_vtbl).set_property)(
        device,
        makediprop(DIPROP_CALIBRATIONMODE),
        &prop.diph,
    );
    check_hresult("IDirectInputDevice8::SetProperty(DIPROP_CALIBRATIONMODE)", hr)
}

/// Reads the live hardware/driver calibration `[lMin, lCenter, lMax]` via
/// `GetProperty(DIPROP_CALIBRATION /* 0x0B */, &mut dipropcal.diph)`.
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn get_axis_calibration(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
) -> CoreResult<AxisCalibration> {
    let mut cal = DipropCal::new(axis_dw_type, DIPH_BYID, 0, 0, 0);
    let hr = ((*(*device).lp_vtbl).get_property)(
        device,
        makediprop(DIPROP_CALIBRATION),
        &mut cal.diph,
    );
    check_hresult("IDirectInputDevice8::GetProperty(DIPROP_CALIBRATION)", hr)?;
    Ok(AxisCalibration::new_unchecked(
        cal.l_min,
        cal.l_center,
        cal.l_max,
    ))
}

/// Writes live calibration `[lMin, lCenter, lMax]` via
/// `SetProperty(DIPROP_CALIBRATION /* 0x0B */, &dipropcal.diph)`.
///
/// As verified in `DIView.exe` (`FUN_00401f20`), `dinput8.dll` immediately persists the 12-byte
/// `REG_BINARY` payload into `HKCU\System\CurrentControlSet\Control\MediaProperties\PrivateProperties\DirectInput`
/// and updates live handles in-memory without requiring USB reconnection.
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn set_axis_calibration(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
    cal: AxisCalibration,
) -> CoreResult<()> {
    let validated = AxisCalibration::new(cal.min, cal.center, cal.max)?;
    let prop = validated.to_dipropcal(axis_dw_type);
    let hr = ((*(*device).lp_vtbl).set_property)(
        device,
        makediprop(DIPROP_CALIBRATION),
        &prop.diph,
    );
    check_hresult("IDirectInputDevice8::SetProperty(DIPROP_CALIBRATION)", hr)
}

/// Reads `DIPROP_DEADZONE` (`5`) in range `0..=10000` (`0%` to `100%`).
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn get_deadzone(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
) -> CoreResult<u32> {
    let mut prop = DipropDword::new(axis_dw_type, DIPH_BYID, 0);
    let hr = ((*(*device).lp_vtbl).get_property)(
        device,
        makediprop(DIPROP_DEADZONE),
        &mut prop.diph,
    );
    check_hresult("IDirectInputDevice8::GetProperty(DIPROP_DEADZONE)", hr)?;
    Ok(prop.dw_data)
}

/// Writes `DIPROP_DEADZONE` (`5`) in range `0..=10000` (`0%` to `100%`).
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn set_deadzone(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
    deadzone: u32,
) -> CoreResult<()> {
    if deadzone > 10_000 {
        return Err(CoreError::InvalidPropertyRange {
            property: "DIPROP_DEADZONE",
            value: deadzone,
            min: 0,
            max: 10_000,
        });
    }
    let prop = DipropDword::new(axis_dw_type, DIPH_BYID, deadzone);
    let hr = ((*(*device).lp_vtbl).set_property)(
        device,
        makediprop(DIPROP_DEADZONE),
        &prop.diph,
    );
    check_hresult("IDirectInputDevice8::SetProperty(DIPROP_DEADZONE)", hr)
}

/// Reads `DIPROP_SATURATION` (`6`) in range `0..=10000` (`0%` to `100%`).
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn get_saturation(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
) -> CoreResult<u32> {
    let mut prop = DipropDword::new(axis_dw_type, DIPH_BYID, 0);
    let hr = ((*(*device).lp_vtbl).get_property)(
        device,
        makediprop(DIPROP_SATURATION),
        &mut prop.diph,
    );
    check_hresult("IDirectInputDevice8::GetProperty(DIPROP_SATURATION)", hr)?;
    Ok(prop.dw_data)
}

/// Writes `DIPROP_SATURATION` (`6`) in range `0..=10000` (`0%` to `100%`).
///
/// # Safety
/// `device` must be a valid, non-null `*mut IDirectInputDevice8W`.
pub unsafe fn set_saturation(
    device: *mut IDirectInputDevice8W,
    axis_dw_type: u32,
    saturation: u32,
) -> CoreResult<()> {
    if saturation > 10_000 {
        return Err(CoreError::InvalidPropertyRange {
            property: "DIPROP_SATURATION",
            value: saturation,
            min: 0,
            max: 10_000,
        });
    }
    let prop = DipropDword::new(axis_dw_type, DIPH_BYID, saturation);
    let hr = ((*(*device).lp_vtbl).set_property)(
        device,
        makediprop(DIPROP_SATURATION),
        &prop.diph,
    );
    check_hresult("IDirectInputDevice8::SetProperty(DIPROP_SATURATION)", hr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axis_calibration_validation_and_default_reset() {
        assert!(AxisCalibration::new(0, 32768, 65535).is_ok());
        assert!(AxisCalibration::new(-32768, 0, 32767).is_ok());

        // Invalid: min >= max or center outside [min, max]
        assert!(AxisCalibration::new(100, 50, 200).is_err());
        assert!(AxisCalibration::new(100, 250, 200).is_err());
        assert!(AxisCalibration::new(200, 200, 200).is_err());

        // DIView FUN_00401f20 default reset formula: min + (max - min + 1) / 2
        let reset_unsigned = AxisCalibration::default_reset(0, 65535);
        assert_eq!(reset_unsigned.min, 0);
        assert_eq!(reset_unsigned.center, 32768);
        assert_eq!(reset_unsigned.max, 65535);

        let reset_signed = AxisCalibration::default_reset(-32768, 32767);
        assert_eq!(reset_signed.min, -32768);
        assert_eq!(reset_signed.center, 0);
        assert_eq!(reset_signed.max, 32767);
    }

    #[test]
    fn test_registry_bytes_roundtrip() {
        let cal = AxisCalibration::new(120, 31500, 64800).unwrap();
        let bytes = cal.to_registry_bytes();
        let decoded = AxisCalibration::from_registry_bytes(&bytes);
        assert_eq!(cal, decoded);
    }

    #[test]
    fn test_diview_raw_normalization_math() {
        let cal = AxisCalibration::new(0, 500, 1000).unwrap();
        let n_min = cal.normalize_raw_diview(0);
        let n_mid = cal.normalize_raw_diview(500);
        let n_max = cal.normalize_raw_diview(1000);

        assert!((n_min - 0.0).abs() < 1e-6);
        assert!((n_mid - 0.5).abs() < 1e-6);
        assert!((n_max - 1.0).abs() < 1e-6);

        // Division-by-zero protection when min == max (unchecked degenerate state)
        let degenerate = AxisCalibration::new_unchecked(500, 500, 500);
        let safe_val = degenerate.normalize_raw_diview(500);
        assert!((safe_val - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_calibration_backup_persistence() {
        let temp_dir = std::env::temp_dir()
            .join("SpeedPrecise_test_backups")
            .join(format!("run_{}", std::process::id()));
        let cal = AxisCalibration::new(0, 32768, 65535).unwrap();
        let backup_path = persist_calibration_backup_in_dir(
            &temp_dir,
            0x046D,
            0xC24F,
            "Logitech G29",
            0x0000_0102,
            cal,
            Some(0),
            Some(10_000),
        )
        .expect("Backup should succeed");

        assert!(backup_path.exists());
        let content = fs::read_to_string(&backup_path).unwrap();
        let parsed: CalibrationBackupRecord = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.vid_hex, "0x046D");
        assert_eq!(parsed.pid_hex, "0xC24F");
        assert_eq!(parsed.previous_calibration, cal);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_smart_pedal_saturation_g920_inverted_math() {
        // Logitech G920 pedal (`0..=255`, inverted: resting = 255, full press = 0)
        // 40% saturation, 0% deadzone -> lMin = 153, lCenter = 204, lMax = 255
        let cal_40 = compute_smart_pedal_calibration(0, 255, 40.0, 0.0, true)
            .expect("40% smart saturation on 0..255 should succeed");
        assert_eq!(cal_40.min, 153);
        assert_eq!(cal_40.center, 204);
        assert_eq!(cal_40.max, 255);

        // 100% saturation, 0% deadzone -> lMin = 0, lCenter = 127, lMax = 255
        let cal_100 = compute_smart_pedal_calibration(0, 255, 100.0, 0.0, true)
            .expect("100% smart saturation on 0..255 should succeed");
        assert_eq!(cal_100.min, 0);
        assert_eq!(cal_100.center, 127);
        assert_eq!(cal_100.max, 255);

        // 60% saturation, 10% initial deadzone on 0..255:
        // lMax = round(255 - 0.10 * 255) = round(229.5) = 230
        // lMin = round(255 - 0.60 * 255) = round(102.0) = 102
        // lCenter = floor((102 + 230) / 2) = 166
        let cal_dz = compute_smart_pedal_calibration(0, 255, 60.0, 10.0, true).unwrap();
        assert_eq!(cal_dz, AxisCalibration::new(102, 166, 230).unwrap());

        // Round-trip slider inference
        let (inferred_sat, inferred_dz) =
            infer_pedal_sliders_from_calibration(0, 255, cal_40, true);
        assert!((inferred_sat - 40.0).abs() < 0.2);
        assert!((inferred_dz - 0.0).abs() < 0.2);

        // User's empirical G920 Brake registry calibration [73, 161, 255] (~71.4% saturation)
        let real_g920_brake = AxisCalibration::new(73, 161, 255).unwrap();
        let (g920_sat, g920_dz) =
            infer_pedal_sliders_from_calibration(0, 255, real_g920_brake, true);
        assert!((g920_sat - 71.4).abs() < 0.2);
        assert_eq!(g920_dz, 0.0);
    }

    #[test]
    fn test_smart_pedal_saturation_non_inverted_and_press_percentages() {
        // Non-inverted 16-bit load-cell pedal (`0..=1000`, resting = 0, full press = 1000)
        let cal = compute_smart_pedal_calibration(0, 1000, 50.0, 5.0, false).unwrap();
        assert_eq!(cal.min, 50);
        assert_eq!(cal.center, 275);
        assert_eq!(cal.max, 500);

        // Inverted G920 pedal telemetry percentages (`0%` bottom at rest, `100%` top when pressed)
        let (phys_rest, game_rest) = compute_pedal_press_percentages(255, (0, 255), 100.0, true);
        assert!((phys_rest - 0.0).abs() < 1e-6);
        assert!((game_rest - 0.0).abs() < 1e-6);

        let (phys_40, game_40) = compute_pedal_press_percentages(153, (0, 255), 0.0, true);
        assert!((phys_40 - 40.0).abs() < 1e-6);
        assert!((game_40 - 100.0).abs() < 1e-6);

        // Steering angle conversion (900° wheel: 0% -> -450°, 50% -> 0°, 100% -> +450°)
        let (deg_left, pct_left) = compute_steering_angle_deg(0.0, 900.0);
        let (deg_center, pct_center) = compute_steering_angle_deg(50.0, 900.0);
        let (deg_right, pct_right) = compute_steering_angle_deg(100.0, 900.0);
        assert!((deg_left - (-450.0)).abs() < 1e-6 && (pct_left - (-100.0)).abs() < 1e-6);
        assert!((deg_center - 0.0).abs() < 1e-6 && (pct_center - 0.0).abs() < 1e-6);
        assert!((deg_right - 450.0).abs() < 1e-6 && (pct_right - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_evaluate_calibrated_output_pct_inverted_g920_and_non_inverted() {
        // 1. G920 Brake (0..=255, inverted: rest = 255, full press = 0) with 40% Saturation, 0% Deadzone -> [153, 204, 255]
        let cal_40 = compute_smart_pedal_calibration(0, 255, 40.0, 0.0, true).unwrap();
        assert_eq!(cal_40, AxisCalibration::new(153, 204, 255).unwrap());

        // Foot off (raw = 255) -> cooked = 100% -> ingame press = 0% (never stuck at 50%!)
        let cooked_rest = evaluate_calibrated_output_pct(255, cal_40, 0, 10_000);
        let (phys_rest, game_rest) =
            compute_pedal_press_percentages(255, (0, 255), cooked_rest, true);
        assert!((phys_rest - 0.0).abs() < 1e-6);
        assert!((game_rest - 0.0).abs() < 1e-6);

        // 20% physical travel (raw = 204) -> 50% in-game output
        let cooked_half = evaluate_calibrated_output_pct(204, cal_40, 0, 10_000);
        let (phys_half, game_half) =
            compute_pedal_press_percentages(204, (0, 255), cooked_half, true);
        assert!((phys_half - 20.0).abs() < 0.2);
        assert!((game_half - 50.0).abs() < 1e-6);

        // 40% physical travel (raw = 153) -> 100% in-game output
        let cooked_sat = evaluate_calibrated_output_pct(153, cal_40, 0, 10_000);
        let (phys_sat, game_sat) =
            compute_pedal_press_percentages(153, (0, 255), cooked_sat, true);
        assert!((phys_sat - 40.0).abs() < 0.2);
        assert!((game_sat - 100.0).abs() < 1e-6);

        // Beyond 40% physical travel (raw = 60) -> clamped strictly at 100% in-game output
        let cooked_beyond = evaluate_calibrated_output_pct(60, cal_40, 0, 10_000);
        let (_, game_beyond) =
            compute_pedal_press_percentages(60, (0, 255), cooked_beyond, true);
        assert!((game_beyond - 100.0).abs() < 1e-6);

        // 2. G920 Brake with 100% Saturation and 30% Initial Deadzone -> [0, 89, 179]
        let cal_dz30 = compute_smart_pedal_calibration(0, 255, 100.0, 30.0, true).unwrap();
        assert_eq!(cal_dz30, AxisCalibration::new(0, 89, 179).unwrap());

        // Inside 0%..30% deadzone (raw = 255, 220, 179): output MUST stay 0%, never 50%!
        for raw in [255, 220, 179] {
            let cooked = evaluate_calibrated_output_pct(raw, cal_dz30, 0, 10_000);
            let (_, game) = compute_pedal_press_percentages(raw, (0, 255), cooked, true);
            assert!(
                game.abs() < 1e-6,
                "Expected 0% output inside 30% deadzone at raw={raw}, got {game}%"
            );
        }

        // Midpoint of active 30%..100% travel (raw = 89) -> 50% output
        let (_, game_mid_dz) = compute_pedal_press_percentages(
            89,
            (0, 255),
            evaluate_calibrated_output_pct(89, cal_dz30, 0, 10_000),
            true,
        );
        assert!((game_mid_dz - 50.0).abs() < 1e-6);

        // 3. Non-inverted 16-bit pedal (0..=1000, S = 50%, D = 10% -> [100, 300, 500])
        let cal_non_inv = compute_smart_pedal_calibration(0, 1000, 50.0, 10.0, false).unwrap();
        assert_eq!(cal_non_inv, AxisCalibration::new(100, 300, 500).unwrap());
        assert!((evaluate_calibrated_output_pct(0, cal_non_inv, 0, 10_000) - 0.0).abs() < 1e-6);
        assert!((evaluate_calibrated_output_pct(80, cal_non_inv, 0, 10_000) - 0.0).abs() < 1e-6);
        assert!((evaluate_calibrated_output_pct(300, cal_non_inv, 0, 10_000) - 50.0).abs() < 1e-6);
        assert!((evaluate_calibrated_output_pct(500, cal_non_inv, 0, 10_000) - 100.0).abs() < 1e-6);
        assert!((evaluate_calibrated_output_pct(900, cal_non_inv, 0, 10_000) - 100.0).abs() < 1e-6);
    }
}
