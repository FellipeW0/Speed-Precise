//! Low-level DirectInput8 (`dinput8.dll`) FFI definitions, COM vtables, property
//! structures, and Force Feedback (`IDirectInputEffect`, `DIEFFECT`, `DICONSTANTFORCE`)
//! verified against the Phase 1 binary audit of `DIView.exe` and `WheelCheck.exe`.

use std::ffi::c_void;
use std::fmt;
use std::mem::{offset_of, size_of};

/// DirectInput 8 version constant (`0x0800`).
pub const DIRECTINPUT_VERSION: u32 = 0x0800;

// ============================================================================
// DirectInput8 Property Header `dwHow` Constants
// ============================================================================
pub const DIPH_DEVICE: u32 = 0;
pub const DIPH_BYOFFSET: u32 = 1;
pub const DIPH_BYID: u32 = 2;
pub const DIPH_BYUSAGE: u32 = 3;

// ============================================================================
// DirectInput8 Property Pseudo-GUIDs (`MAKEDIPROP`)
// Verified in Phase 1 audit (`DIView.exe` & `WheelCheck.exe` calls to GetProperty/SetProperty)
// ============================================================================
pub const DIPROP_BUFFERSIZE: usize = 1; // 0x01
pub const DIPROP_AXISMODE: usize = 2; // 0x02
pub const DIPROP_GRANULARITY: usize = 3; // 0x03
pub const DIPROP_RANGE: usize = 4; // 0x04
pub const DIPROP_DEADZONE: usize = 5; // 0x05
pub const DIPROP_SATURATION: usize = 6; // 0x06
pub const DIPROP_FFGAIN: usize = 7; // 0x07
pub const DIPROP_FFLOAD: usize = 8; // 0x08
pub const DIPROP_AUTOCENTER: usize = 9; // 0x09
pub const DIPROP_CALIBRATIONMODE: usize = 10; // 0x0A
pub const DIPROP_CALIBRATION: usize = 11; // 0x0B
pub const DIPROP_GUIDANDPATH: usize = 12; // 0x0C
pub const DIPROP_PHYSICALRANGE: usize = 18; // 0x12
pub const DIPROP_LOGICALRANGE: usize = 19; // 0x13

/// Calibration mode constants for `DIPROP_CALIBRATIONMODE` (`0x0A`).
pub const DIPROPCALIBRATIONMODE_COOKED: u32 = 0;
pub const DIPROPCALIBRATIONMODE_RAW: u32 = 1;

/// Axis mode constants for `DIPROP_AXISMODE` (`0x02`).
pub const DIPROPAXISMODE_ABS: u32 = 0;
pub const DIPROPAXISMODE_REL: u32 = 1;

// ============================================================================
// DirectInput8 Device Enumeration, Cooperative Level & Object Type Flags
// ============================================================================
pub const DI8DEVCLASS_ALL: u32 = 0;
pub const DI8DEVCLASS_GAMECTRL: u32 = 4;

pub const DIEDFL_ALLDEVICES: u32 = 0x0000_0000;
pub const DIEDFL_ATTACHEDONLY: u32 = 0x0000_0001;
pub const DIEDFL_FORCEFEEDBACK: u32 = 0x0000_0100;

pub const DIENUM_STOP: i32 = 0;
pub const DIENUM_CONTINUE: i32 = 1;

pub const DISCL_EXCLUSIVE: u32 = 0x0000_0001;
pub const DISCL_NONEXCLUSIVE: u32 = 0x0000_0002;
pub const DISCL_FOREGROUND: u32 = 0x0000_0004;
pub const DISCL_BACKGROUND: u32 = 0x0000_0008;
pub const DISCL_NOWINKEY: u32 = 0x0000_0010;

pub const DIDFT_ALL: u32 = 0x0000_0000;
pub const DIDFT_RELAXIS: u32 = 0x0000_0001;
pub const DIDFT_ABSAXIS: u32 = 0x0000_0002;
pub const DIDFT_AXIS: u32 = 0x0000_0003;
pub const DIDFT_PSHBUTTON: u32 = 0x0000_0004;
pub const DIDFT_TGLBUTTON: u32 = 0x0000_0008;
pub const DIDFT_BUTTON: u32 = 0x0000_000C;
pub const DIDFT_POV: u32 = 0x0000_0010;
pub const DIDFT_COLLECTION: u32 = 0x0000_0040;
pub const DIDFT_NODATA: u32 = 0x0000_0080;
pub const DIDFT_ANYINSTANCE: u32 = 0x00FF_FF00;
pub const DIDFT_FFACTUATOR: u32 = 0x0100_0000;
pub const DIDFT_OPTIONAL: u32 = 0x8000_0000;

pub const DIDC_ATTACHED: u32 = 0x0000_0001;
pub const DIDC_POLLEDDEVICE: u32 = 0x0000_0002;
pub const DIDC_EMULATED: u32 = 0x0000_0004;
pub const DIDC_POLLEDDATAFORMAT: u32 = 0x0000_0008;
pub const DIDC_FORCEFEEDBACK: u32 = 0x0000_0100;

pub const DIDF_ABSAXIS: u32 = 0x0000_0001;
pub const DIDF_RELAXIS: u32 = 0x0000_0002;

// ============================================================================
// DirectInput8 Force Feedback (`DIEFF_*`, `DIEP_*`, `DISFFC_*`) Constants
// ============================================================================
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const DI_DEGREES: u32 = 100;
pub const DI_FFNOMINALMAX: u32 = 10_000;
pub const DI_SECONDS: u32 = 1_000_000;

pub const DIEFF_OBJECTIDS: u32 = 0x0000_0001;
pub const DIEFF_OBJECTOFFSETS: u32 = 0x0000_0002;
pub const DIEFF_CARTESIAN: u32 = 0x0000_0010;
pub const DIEFF_POLAR: u32 = 0x0000_0020;
pub const DIEFF_SPHERICAL: u32 = 0x0000_0040;

pub const DIEP_DURATION: u32 = 0x0000_0001;
pub const DIEP_SAMPLEPERIOD: u32 = 0x0000_0002;
pub const DIEP_GAIN: u32 = 0x0000_0004;
pub const DIEP_TRIGGERBUTTON: u32 = 0x0000_0008;
pub const DIEP_TRIGGERREPEATINTERVAL: u32 = 0x0000_0010;
pub const DIEP_AXES: u32 = 0x0000_0020;
pub const DIEP_DIRECTION: u32 = 0x0000_0040;
pub const DIEP_ENVELOPE: u32 = 0x0000_0080;
pub const DIEP_TYPESPECIFICPARAMS: u32 = 0x0000_0100;
pub const DIEP_STARTDELAY: u32 = 0x0000_0200;
pub const DIEP_ALLPARAMS_DX5: u32 = 0x0000_01FF;
pub const DIEP_ALLPARAMS: u32 = 0x0000_03FF;
pub const DIEP_START: u32 = 0x2000_0000;
pub const DIEP_NORESTART: u32 = 0x4000_0000;
pub const DIEP_NODOWNLOAD: u32 = 0x8000_0000;

pub const DIEB_NOTRIGGER: u32 = 0xFFFF_FFFF;

pub const DIES_SOLO: u32 = 0x0000_0001;
pub const DIES_NODOWNLOAD: u32 = 0x8000_0000;

pub const DISFFC_RESET: u32 = 0x0000_0001;
pub const DISFFC_STOPALL: u32 = 0x0000_0002;
pub const DISFFC_PAUSE: u32 = 0x0000_0004;
pub const DISFFC_CONTINUE: u32 = 0x0000_0008;
pub const DISFFC_SETACTUATORSON: u32 = 0x0000_0010;
pub const DISFFC_SETACTUATORSOFF: u32 = 0x0000_0020;

// ============================================================================
// Win32 Multimedia Timer (`winmm.dll`) & Scheduler Priority (`kernel32.dll`)
// Verified in `WheelCheck.exe` (`FUN_004024d0`, `FUN_00401210`, `FUN_004078c0`)
// ============================================================================
pub const TIMERR_NOERROR: u32 = 0;
pub const NORMAL_PRIORITY_CLASS: u32 = 0x0000_0020;
pub const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
pub const REALTIME_PRIORITY_CLASS: u32 = 0x0000_0100;
pub const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
pub const TIMER_ALL_ACCESS: u32 = 0x001F_0003;

/// Converts a DirectInput property ID (`1..=19`) into a pseudo-GUID pointer (`MAKEDIPROP`).
#[inline]
pub const fn makediprop(prop_id: usize) -> *const Guid {
    prop_id as *const Guid
}

/// Extracts the 16-bit instance index from a DirectInput `dwType` field (`DIDFT_GETINSTANCE`).
#[inline]
pub const fn didft_get_instance(dw_type: u32) -> u16 {
    ((dw_type >> 8) & 0xFFFF) as u16
}

/// Extracts the 8-bit base type flags from a DirectInput `dwType` field (`DIDFT_GETTYPE`).
#[inline]
pub const fn didft_get_type(dw_type: u32) -> u8 {
    (dw_type & 0xFF) as u8
}

// ============================================================================
// GUID Definition & Known DirectInput8 GUIDs
// ============================================================================
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }

    /// Extracts `(VID, PID)` encoded in the lower/upper 16 bits of `guidProduct.Data1`.
    #[inline]
    pub const fn vid_pid(&self) -> (u16, u16) {
        let vid = (self.data1 & 0xFFFF) as u16;
        let pid = ((self.data1 >> 16) & 0xFFFF) as u16;
        (vid, pid)
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )
    }
}

/// `IID_IDirectInput8W` = `{BF798031-483A-4DA2-AA99-5D64ED369700}`
pub const IID_IDIRECTINPUT8W: Guid = Guid::new(
    0xBF79_8031,
    0x483A,
    0x4DA2,
    [0xAA, 0x99, 0x5D, 0x64, 0xED, 0x36, 0x97, 0x00],
);

/// `GUID_XAxis` = `{A36D02E0-C9F3-11CF-BFC7-444553540000}`
pub const GUID_XAXIS: Guid = Guid::new(
    0xA36D_02E0,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_YAxis` = `{A36D02E1-C9F3-11CF-BFC7-444553540000}`
pub const GUID_YAXIS: Guid = Guid::new(
    0xA36D_02E1,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_ZAxis` = `{A36D02E2-C9F3-11CF-BFC7-444553540000}`
pub const GUID_ZAXIS: Guid = Guid::new(
    0xA36D_02E2,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_RxAxis` = `{A36D02F4-C9F3-11CF-BFC7-444553540000}`
pub const GUID_RXAXIS: Guid = Guid::new(
    0xA36D_02F4,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_RyAxis` = `{A36D02F5-C9F3-11CF-BFC7-444553540000}`
pub const GUID_RYAXIS: Guid = Guid::new(
    0xA36D_02F5,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_RzAxis` = `{A36D02E3-C9F3-11CF-BFC7-444553540000}`
pub const GUID_RZAXIS: Guid = Guid::new(
    0xA36D_02E3,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_Slider` = `{A36D02E4-C9F3-11CF-BFC7-444553540000}`
pub const GUID_SLIDER: Guid = Guid::new(
    0xA36D_02E4,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_POV` = `{A36D02F2-C9F3-11CF-BFC7-444553540000}`
pub const GUID_POV: Guid = Guid::new(
    0xA36D_02F2,
    0xC9F3,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// `GUID_ConstantForce` = `{13541C20-8E33-11D0-9AD0-00A0C9A06E35}`
/// Verified at `0x004373D0` in `WheelCheck.exe`.
pub const GUID_CONSTANTFORCE: Guid = Guid::new(
    0x1354_1C20,
    0x8E33,
    0x11D0,
    [0x9A, 0xD0, 0x00, 0xA0, 0xC9, 0xA0, 0x6E, 0x35],
);

/// `GUID_Spring` = `{13541C27-8E33-11D0-9AD0-00A0C9A06E35}`
pub const GUID_SPRING: Guid = Guid::new(
    0x1354_1C27,
    0x8E33,
    0x11D0,
    [0x9A, 0xD0, 0x00, 0xA0, 0xC9, 0xA0, 0x6E, 0x35],
);

// ============================================================================
// DirectInput8 Property Structs (Verified Exact Binary Layouts)
// ============================================================================

/// `DIPROPHEADER` (`16` bytes / `0x10`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DipropHeader {
    pub dw_size: u32,
    pub dw_header_size: u32,
    pub dw_obj: u32,
    pub dw_how: u32,
}

impl DipropHeader {
    pub const SIZE: u32 = 16;

    #[inline]
    pub const fn new(dw_size: u32, dw_obj: u32, dw_how: u32) -> Self {
        Self {
            dw_size,
            dw_header_size: Self::SIZE,
            dw_obj,
            dw_how,
        }
    }
}

/// `DIPROPDWORD` (`20` bytes / `0x14`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DipropDword {
    pub diph: DipropHeader,
    pub dw_data: u32,
}

impl DipropDword {
    pub const SIZE: u32 = 20;

    #[inline]
    pub const fn new(dw_obj: u32, dw_how: u32, dw_data: u32) -> Self {
        Self {
            diph: DipropHeader::new(Self::SIZE, dw_obj, dw_how),
            dw_data,
        }
    }
}

/// `DIPROPRANGE` (`24` bytes / `0x18`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DipropRange {
    pub diph: DipropHeader,
    pub l_min: i32,
    pub l_max: i32,
}

impl DipropRange {
    pub const SIZE: u32 = 24;

    #[inline]
    pub const fn new(dw_obj: u32, dw_how: u32, l_min: i32, l_max: i32) -> Self {
        Self {
            diph: DipropHeader::new(Self::SIZE, dw_obj, dw_how),
            l_min,
            l_max,
        }
    }
}

/// `DIPROPCAL` (`28` bytes / `0x1C`) used with `DIPROP_CALIBRATION` (`11` / `0x0B`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DipropCal {
    pub diph: DipropHeader,
    pub l_min: i32,
    pub l_center: i32,
    pub l_max: i32,
}

impl DipropCal {
    pub const SIZE: u32 = 28;

    #[inline]
    pub const fn new(dw_obj: u32, dw_how: u32, l_min: i32, l_center: i32, l_max: i32) -> Self {
        Self {
            diph: DipropHeader::new(Self::SIZE, dw_obj, dw_how),
            l_min,
            l_center,
            l_max,
        }
    }

    /// Serializes `[l_min, l_center, l_max]` into the exact 12-byte Little-Endian `REG_BINARY`
    /// payload written to `HKCU\...\DirectInput\VID_%04X&PID_%04X\Calibration\0\Type\Axes\%d`.
    #[inline]
    pub const fn to_registry_bytes(&self) -> [u8; 12] {
        let min_b = self.l_min.to_le_bytes();
        let ctr_b = self.l_center.to_le_bytes();
        let max_b = self.l_max.to_le_bytes();
        [
            min_b[0], min_b[1], min_b[2], min_b[3], ctr_b[0], ctr_b[1], ctr_b[2], ctr_b[3],
            max_b[0], max_b[1], max_b[2], max_b[3],
        ]
    }
}

/// `DIPROPGUIDANDPATH` (`552` bytes / `0x228`) used with `DIPROP_GUIDANDPATH` (`12` / `0x0C`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DipropGuidAndPath {
    pub diph: DipropHeader,
    pub guid_class: Guid,
    pub wsz_path: [u16; 260],
}

impl DipropGuidAndPath {
    pub const SIZE: u32 = 552;

    pub fn new() -> Self {
        Self {
            diph: DipropHeader::new(Self::SIZE, 0, DIPH_DEVICE),
            guid_class: Guid::default(),
            wsz_path: [0u16; 260],
        }
    }
}

impl Default for DipropGuidAndPath {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Force Feedback (`DICONSTANTFORCE`, `DIENVELOPE`, `DIEFFECT`) Structs
// ============================================================================

/// `DICONSTANTFORCE` (`4` bytes): constant torque magnitude in `[-10000, +10000]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiConstantForce {
    pub l_magnitude: i32,
}

impl DiConstantForce {
    pub const SIZE: u32 = 4;

    #[inline]
    pub const fn new(l_magnitude: i32) -> Self {
        Self { l_magnitude }
    }
}

/// `DIENVELOPE` (`20` bytes / `0x14`): optional attack/fade envelope for FFB effects.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiEnvelope {
    pub dw_size: u32,
    pub dw_attack_level: u32,
    pub dw_attack_time: u32,
    pub dw_fade_level: u32,
    pub dw_fade_time: u32,
}

impl DiEnvelope {
    pub const SIZE: u32 = 20;

    #[inline]
    pub const fn new(
        dw_attack_level: u32,
        dw_attack_time: u32,
        dw_fade_level: u32,
        dw_fade_time: u32,
    ) -> Self {
        Self {
            dw_size: Self::SIZE,
            dw_attack_level,
            dw_attack_time,
            dw_fade_level,
            dw_fade_time,
        }
    }
}

/// `DIEFFECT` (`80` bytes on `x86_64`, `56` bytes on `x86`).
///
/// Matches the exact layout expected by `IDirectInputDevice8::CreateEffect` and
/// `IDirectInputEffect::SetParameters` in `WheelCheck.exe` (`FUN_004034a0`, lines 2815-2850).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiEffect {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_duration: u32,
    pub dw_sample_period: u32,
    pub dw_gain: u32,
    pub dw_trigger_button: u32,
    pub dw_trigger_repeat_interval: u32,
    pub c_axes: u32,
    pub rgdw_axes: *mut u32,
    pub rgl_direction: *mut i32,
    pub lp_envelope: *mut DiEnvelope,
    pub cb_type_specific_params: u32,
    pub lpv_type_specific_params: *mut c_void,
    pub dw_start_delay: u32,
}

impl DiEffect {
    /// Constructs a `DIEFFECT` configured for a single-axis `GUID_ConstantForce` effect
    /// matching `WheelCheck.exe` (`FUN_004034a0`):
    /// - `dw_flags = DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS` (`0x12`)
    /// - `dw_duration = INFINITE` (`0xFFFFFFFF`)
    /// - `dw_gain = DI_FFNOMINALMAX` (`10000`)
    /// - `dw_trigger_button = DIEB_NOTRIGGER` (`0xFFFFFFFF`)
    /// - `c_axes = 1`
    pub fn new_constant_force(
        axis_ptr: *mut u32,
        dir_ptr: *mut i32,
        cf_ptr: *mut DiConstantForce,
        use_object_ids: bool,
    ) -> Self {
        let axis_flag = if use_object_ids {
            DIEFF_OBJECTIDS
        } else {
            DIEFF_OBJECTOFFSETS
        };
        Self {
            dw_size: size_of::<Self>() as u32,
            dw_flags: DIEFF_CARTESIAN | axis_flag,
            dw_duration: INFINITE,
            dw_sample_period: 0,
            dw_gain: DI_FFNOMINALMAX,
            dw_trigger_button: DIEB_NOTRIGGER,
            dw_trigger_repeat_interval: 0,
            c_axes: 1,
            rgdw_axes: axis_ptr,
            rgl_direction: dir_ptr,
            lp_envelope: std::ptr::null_mut(),
            cb_type_specific_params: size_of::<DiConstantForce>() as u32,
            lpv_type_specific_params: cf_ptr.cast::<c_void>(),
            dw_start_delay: 0,
        }
    }
}

// Compile-time size assertions for all DirectInput8 property and FFB structs
const _: () = {
    assert!(size_of::<Guid>() == 16);
    assert!(size_of::<DipropHeader>() == 16);
    assert!(size_of::<DipropDword>() == 20);
    assert!(size_of::<DipropRange>() == 24);
    assert!(size_of::<DipropCal>() == 28);
    assert!(size_of::<DipropGuidAndPath>() == 552);
    assert!(offset_of!(DipropCal, l_min) == 16);
    assert!(offset_of!(DipropCal, l_center) == 20);
    assert!(offset_of!(DipropCal, l_max) == 24);

    assert!(size_of::<DiConstantForce>() == 4);
    assert!(size_of::<DiEnvelope>() == 20);
    #[cfg(target_pointer_width = "64")]
    {
        assert!(size_of::<DiEffect>() == 80);
        assert!(offset_of!(DiEffect, rgdw_axes) == 32);
        assert!(offset_of!(DiEffect, rgl_direction) == 40);
        assert!(offset_of!(DiEffect, lp_envelope) == 48);
        assert!(offset_of!(DiEffect, cb_type_specific_params) == 56);
        assert!(offset_of!(DiEffect, lpv_type_specific_params) == 64);
        assert!(offset_of!(DiEffect, dw_start_delay) == 72);
    }
    #[cfg(target_pointer_width = "32")]
    {
        assert!(size_of::<DiEffect>() == 56);
    }
};

// ============================================================================
// DirectInput8 Device Instance, Object Instance, Capabilities & State Structs
// ============================================================================

/// `DIDEVCAPS` (`44` bytes / `0x2C`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DiDevCaps {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_dev_type: u32,
    pub dw_axes: u32,
    pub dw_buttons: u32,
    pub dw_povs: u32,
    pub dw_ff_sample_period: u32,
    pub dw_ff_min_time_resolution: u32,
    pub dw_firmware_revision: u32,
    pub dw_hardware_revision: u32,
    pub dw_ff_driver_version: u32,
}

impl DiDevCaps {
    pub const SIZE: u32 = 44;

    pub const fn new() -> Self {
        Self {
            dw_size: Self::SIZE,
            dw_flags: 0,
            dw_dev_type: 0,
            dw_axes: 0,
            dw_buttons: 0,
            dw_povs: 0,
            dw_ff_sample_period: 0,
            dw_ff_min_time_resolution: 0,
            dw_firmware_revision: 0,
            dw_hardware_revision: 0,
            dw_ff_driver_version: 0,
        }
    }
}

/// `DIDEVICEINSTANCEW` (`1100` bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DiDeviceInstanceW {
    pub dw_size: u32,
    pub guid_instance: Guid,
    pub guid_product: Guid,
    pub dw_dev_type: u32,
    pub tsz_instance_name: [u16; 260],
    pub tsz_product_name: [u16; 260],
    pub guid_ff_driver: Guid,
    pub w_usage_page: u16,
    pub w_usage: u16,
}

impl DiDeviceInstanceW {
    pub fn instance_name(&self) -> String {
        utf16_nul_to_string(&self.tsz_instance_name)
    }

    pub fn product_name(&self) -> String {
        utf16_nul_to_string(&self.tsz_product_name)
    }
}

/// `DIDEVICEOBJECTINSTANCEW` (`576` bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DiDeviceObjectInstanceW {
    pub dw_size: u32,
    pub guid_type: Guid,
    pub dw_ofs: u32,
    pub dw_type: u32,
    pub dw_flags: u32,
    pub tsz_name: [u16; 260],
    pub dw_ff_max_force: u32,
    pub dw_ff_force_resolution: u32,
    pub w_collection_number: u16,
    pub w_designator_index: u16,
    pub w_usage_page: u16,
    pub w_usage: u16,
    pub dw_dimension: u32,
    pub w_exponent: u16,
    pub w_report_id: u16,
}

impl DiDeviceObjectInstanceW {
    pub fn name(&self) -> String {
        utf16_nul_to_string(&self.tsz_name)
    }
}

/// `DIJOYSTATE` (`80` bytes / `0x50`) standard joystick state structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiJoyState {
    pub l_x: i32,
    pub l_y: i32,
    pub l_z: i32,
    pub l_rx: i32,
    pub l_ry: i32,
    pub l_rz: i32,
    pub rgl_slider: [i32; 2],
    pub rgdw_pov: [u32; 4],
    pub rgb_buttons: [u8; 32],
}

/// `DIOBJECTDATAFORMAT` (`24` bytes on x86_64, `16` bytes on x86).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiObjectDataFormat {
    pub pguid: *const Guid,
    pub dw_ofs: u32,
    pub dw_type: u32,
    pub dw_flags: u32,
}

// Safety: `pguid` points only to static GUID constants or null.
unsafe impl Sync for DiObjectDataFormat {}
unsafe impl Send for DiObjectDataFormat {}

/// `DIDATAFORMAT` (`32` bytes on x86_64, `24` bytes on x86).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiDataFormat {
    pub dw_size: u32,
    pub dw_obj_size: u32,
    pub dw_flags: u32,
    pub dw_data_size: u32,
    pub dw_num_objs: u32,
    pub rgodf: *const DiObjectDataFormat,
}

const _: () = {
    assert!(size_of::<DiDevCaps>() == 44);
    assert!(size_of::<DiDeviceInstanceW>() == 1100);
    assert!(size_of::<DiDeviceObjectInstanceW>() == 576);
    assert!(size_of::<DiJoyState>() == 80);
};

// ============================================================================
// COM Interface VTables (`IDirectInput8W`, `IDirectInputDevice8W`, `IDirectInputEffect`)
// ============================================================================

pub type DiEnumDevicesCallbackW =
    unsafe extern "system" fn(lpddi: *const DiDeviceInstanceW, pv_ref: *mut c_void) -> i32;

pub type DiEnumDeviceObjectsCallbackW =
    unsafe extern "system" fn(lpddoi: *const DiDeviceObjectInstanceW, pv_ref: *mut c_void) -> i32;

#[repr(C)]
pub struct IDirectInput8W {
    pub lp_vtbl: *const IDirectInput8WVtbl,
}

#[repr(C)]
pub struct IDirectInput8WVtbl {
    // IUnknown (Slots 0..=2)
    pub query_interface: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        riid: *const Guid,
        ppv_obj: *mut *mut c_void,
    ) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut IDirectInput8W) -> u32,
    pub release: unsafe extern "system" fn(this: *mut IDirectInput8W) -> u32,

    // IDirectInput8W (Slots 3..=10)
    pub create_device: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        rguid: *const Guid,
        lplp_direct_input_device: *mut *mut IDirectInputDevice8W,
        p_unk_outer: *mut c_void,
    ) -> i32,
    pub enum_devices: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        dw_dev_type: u32,
        lp_callback: DiEnumDevicesCallbackW,
        pv_ref: *mut c_void,
        dw_flags: u32,
    ) -> i32,
    pub get_device_status:
        unsafe extern "system" fn(this: *mut IDirectInput8W, rguid_instance: *const Guid) -> i32,
    pub run_control_panel: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        hwnd_owner: *mut c_void,
        dw_flags: u32,
    ) -> i32,
    pub initialize: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        hinst: *mut c_void,
        dw_version: u32,
    ) -> i32,
    pub find_device: unsafe extern "system" fn(
        this: *mut IDirectInput8W,
        rguid_class: *const Guid,
        ptsz_name: *const u16,
        pguid_instance: *mut Guid,
    ) -> i32,
    pub enum_devices_by_semantics: *const c_void,
    pub configure_devices: *const c_void,
}

#[repr(C)]
pub struct IDirectInputDevice8W {
    pub lp_vtbl: *const IDirectInputDevice8WVtbl,
}

#[repr(C)]
pub struct IDirectInputDevice8WVtbl {
    // IUnknown (Slots 0..=2; x86 offsets 0x00, 0x04, 0x08)
    pub query_interface: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        riid: *const Guid,
        ppv_obj: *mut *mut c_void,
    ) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut IDirectInputDevice8W) -> u32,
    pub release: unsafe extern "system" fn(this: *mut IDirectInputDevice8W) -> u32,

    // IDirectInputDevice8W (Slots 3..=31; x86 offsets 0x0C..=0x7C)
    pub get_capabilities: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        lp_di_dev_caps: *mut DiDevCaps,
    ) -> i32,
    pub enum_objects: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        lp_callback: DiEnumDeviceObjectsCallbackW,
        pv_ref: *mut c_void,
        dw_flags: u32,
    ) -> i32,
    pub get_property: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        rguid_prop: *const Guid,
        pdiph: *mut DipropHeader,
    ) -> i32,
    pub set_property: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        rguid_prop: *const Guid,
        pdiph: *const DipropHeader,
    ) -> i32,
    pub acquire: unsafe extern "system" fn(this: *mut IDirectInputDevice8W) -> i32,
    pub unacquire: unsafe extern "system" fn(this: *mut IDirectInputDevice8W) -> i32,
    pub get_device_state: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        cb_data: u32,
        lpv_data: *mut c_void,
    ) -> i32,
    pub get_device_data: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        cb_object_data: u32,
        rgdod: *mut c_void,
        pdw_in_out: *mut u32,
        dw_flags: u32,
    ) -> i32,
    pub set_data_format: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        lpdf: *const DiDataFormat,
    ) -> i32,
    pub set_event_notification:
        unsafe extern "system" fn(this: *mut IDirectInputDevice8W, h_event: *mut c_void) -> i32,
    pub set_cooperative_level: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        hwnd: *mut c_void,
        dw_flags: u32,
    ) -> i32,
    pub get_object_info: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        pdidoi: *mut DiDeviceObjectInstanceW,
        dw_obj: u32,
        dw_how: u32,
    ) -> i32,
    pub get_device_info: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        pdidi: *mut DiDeviceInstanceW,
    ) -> i32,
    pub run_control_panel: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        hwnd_owner: *mut c_void,
        dw_flags: u32,
    ) -> i32,
    pub initialize: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        hinst: *mut c_void,
        dw_version: u32,
        rguid: *const Guid,
    ) -> i32,
    pub create_effect: unsafe extern "system" fn(
        this: *mut IDirectInputDevice8W,
        rguid: *const Guid,
        lpeff: *const DiEffect,
        ppdeff: *mut *mut IDirectInputEffect,
        punk_outer: *mut c_void,
    ) -> i32,
    pub enum_effects: *const c_void,
    pub get_effect_info: *const c_void,
    pub get_force_feedback_state: *const c_void,
    pub send_force_feedback_command:
        unsafe extern "system" fn(this: *mut IDirectInputDevice8W, dw_flags: u32) -> i32,
    pub enum_created_effect_objects: *const c_void,
    pub escape: *const c_void,
    pub poll: unsafe extern "system" fn(this: *mut IDirectInputDevice8W) -> i32,
    pub send_device_data: *const c_void,
    pub enum_effects_in_file: *const c_void,
    pub write_effect_to_file: *const c_void,
    pub build_action_map: *const c_void,
    pub set_action_map: *const c_void,
    pub get_image_info: *const c_void,
}

#[repr(C)]
pub struct IDirectInputEffect {
    pub lp_vtbl: *const IDirectInputEffectVtbl,
}

/// VTable for `IDirectInputEffect` (Slots `0..=12`; x86 offsets `0x00..=0x30`).
/// Verified against `WheelCheck.exe` calls to `SetParameters` (`+0x18`, slot 6),
/// `Start` (`+0x1C`, slot 7), and `Stop` (`+0x20`, slot 8).
#[repr(C)]
pub struct IDirectInputEffectVtbl {
    // IUnknown (Slots 0..=2)
    pub query_interface: unsafe extern "system" fn(
        this: *mut IDirectInputEffect,
        riid: *const Guid,
        ppv_obj: *mut *mut c_void,
    ) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut IDirectInputEffect) -> u32,
    pub release: unsafe extern "system" fn(this: *mut IDirectInputEffect) -> u32,

    // IDirectInputEffect (Slots 3..=12)
    pub initialize: unsafe extern "system" fn(
        this: *mut IDirectInputEffect,
        hinst: *mut c_void,
        dw_version: u32,
        rguid: *const Guid,
    ) -> i32,
    pub get_effect_guid:
        unsafe extern "system" fn(this: *mut IDirectInputEffect, pguid: *mut Guid) -> i32,
    pub get_parameters: unsafe extern "system" fn(
        this: *mut IDirectInputEffect,
        peff: *mut DiEffect,
        dw_flags: u32,
    ) -> i32,
    pub set_parameters: unsafe extern "system" fn(
        this: *mut IDirectInputEffect,
        peff: *const DiEffect,
        dw_flags: u32,
    ) -> i32,
    pub start: unsafe extern "system" fn(
        this: *mut IDirectInputEffect,
        dw_iterations: u32,
        dw_flags: u32,
    ) -> i32,
    pub stop: unsafe extern "system" fn(this: *mut IDirectInputEffect) -> i32,
    pub get_effect_status:
        unsafe extern "system" fn(this: *mut IDirectInputEffect, pdw_flags: *mut u32) -> i32,
    pub download: unsafe extern "system" fn(this: *mut IDirectInputEffect) -> i32,
    pub unload: unsafe extern "system" fn(this: *mut IDirectInputEffect) -> i32,
    pub escape:
        unsafe extern "system" fn(this: *mut IDirectInputEffect, pesc: *mut c_void) -> i32,
}

#[link(name = "dinput8")]
extern "system" {
    pub fn DirectInput8Create(
        hinst: *mut c_void,
        dw_version: u32,
        riidltf: *const Guid,
        ppv_out: *mut *mut c_void,
        punk_outer: *mut c_void,
    ) -> i32;
}

#[link(name = "winmm")]
extern "system" {
    pub fn timeBeginPeriod(u_period: u32) -> u32;
    pub fn timeEndPeriod(u_period: u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(lp_module_name: *const u16) -> *mut c_void;
    pub fn GetConsoleWindow() -> *mut c_void;
    pub fn GetCurrentProcess() -> *mut c_void;
    pub fn GetPriorityClass(h_process: *mut c_void) -> u32;
    pub fn SetPriorityClass(h_process: *mut c_void, dw_priority_class: u32) -> i32;
    pub fn GetCurrentThread() -> *mut c_void;
    pub fn SetThreadAffinityMask(h_thread: *mut c_void, dw_thread_affinity_mask: usize) -> usize;
    pub fn CreateWaitableTimerExW(
        lp_timer_attributes: *const c_void,
        lp_timer_name: *const u16,
        dw_flags: u32,
        dw_desired_access: u32,
    ) -> *mut c_void;
    pub fn SetWaitableTimer(
        h_timer: *mut c_void,
        lp_due_time: *const i64,
        l_period: i32,
        pfn_completion_routine: *const c_void,
        lp_arg_to_completion_routine: *mut c_void,
        f_resume: i32,
    ) -> i32;
    pub fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
    pub fn CloseHandle(h_object: *mut c_void) -> i32;
    pub fn Sleep(dw_milliseconds: u32);
}

#[link(name = "user32")]
extern "system" {
    pub fn CreateWindowExW(
        dw_ex_style: u32,
        lp_class_name: *const u16,
        lp_window_name: *const u16,
        dw_style: u32,
        x: i32,
        y: i32,
        n_width: i32,
        n_height: i32,
        h_wnd_parent: *mut c_void,
        h_menu: *mut c_void,
        h_instance: *mut c_void,
        lp_param: *mut c_void,
    ) -> *mut c_void;
    pub fn DestroyWindow(h_wnd: *mut c_void) -> i32;
    pub fn GetDesktopWindow() -> *mut c_void;
    pub fn GetForegroundWindow() -> *mut c_void;
}

/// Helper to convert a UTF-16 NUL-terminated fixed array into a Rust `String`.
pub fn utf16_nul_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

/// Returns a human-readable standard axis label for known DirectInput axis GUIDs.
pub fn axis_guid_name(guid: &Guid) -> &'static str {
    if *guid == GUID_XAXIS {
        "X Axis"
    } else if *guid == GUID_YAXIS {
        "Y Axis"
    } else if *guid == GUID_ZAXIS {
        "Z Axis"
    } else if *guid == GUID_RXAXIS {
        "Rx Axis"
    } else if *guid == GUID_RYAXIS {
        "Ry Axis"
    } else if *guid == GUID_RZAXIS {
        "Rz Axis"
    } else if *guid == GUID_SLIDER {
        "Slider"
    } else if *guid == GUID_POV {
        "POV"
    } else {
        "Custom Axis"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn test_property_struct_sizes_and_offsets() {
        assert_eq!(size_of::<DipropHeader>(), 16, "DIPROPHEADER must be 16 bytes (0x10)");
        assert_eq!(size_of::<DipropDword>(), 20, "DIPROPDWORD must be 20 bytes (0x14)");
        assert_eq!(size_of::<DipropRange>(), 24, "DIPROPRANGE must be 24 bytes (0x18)");
        assert_eq!(size_of::<DipropCal>(), 28, "DIPROPCAL must be 28 bytes (0x1C)");
        assert_eq!(
            size_of::<DipropGuidAndPath>(),
            552,
            "DIPROPGUIDANDPATH must be 552 bytes (0x228)"
        );

        assert_eq!(offset_of!(DipropHeader, dw_size), 0);
        assert_eq!(offset_of!(DipropHeader, dw_header_size), 4);
        assert_eq!(offset_of!(DipropHeader, dw_obj), 8);
        assert_eq!(offset_of!(DipropHeader, dw_how), 12);

        assert_eq!(offset_of!(DipropDword, dw_data), 16);
        assert_eq!(offset_of!(DipropRange, l_min), 16);
        assert_eq!(offset_of!(DipropRange, l_max), 20);

        assert_eq!(offset_of!(DipropCal, l_min), 16);
        assert_eq!(offset_of!(DipropCal, l_center), 20);
        assert_eq!(offset_of!(DipropCal, l_max), 24);
    }

    #[test]
    fn test_ffb_struct_sizes_and_pointer_alignment() {
        assert_eq!(size_of::<DiConstantForce>(), 4);
        assert_eq!(size_of::<DiEnvelope>(), 20);

        let mut axis: u32 = 0;
        let mut dir: i32 = 0;
        let mut cf = DiConstantForce::new(-2500);
        let eff = DiEffect::new_constant_force(&mut axis, &mut dir, &mut cf, false);

        assert_eq!(eff.dw_size, size_of::<DiEffect>() as u32);
        assert_eq!(eff.dw_flags, DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS);
        assert_eq!(eff.dw_duration, INFINITE);
        assert_eq!(eff.dw_gain, 10_000);
        assert_eq!(eff.c_axes, 1);
        assert_eq!(eff.cb_type_specific_params, 4);

        let ptr_size = size_of::<usize>();
        assert_eq!(offset_of!(IDirectInputEffectVtbl, initialize), 3 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, get_effect_guid), 4 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, get_parameters), 5 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, set_parameters), 6 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, start), 7 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, stop), 8 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, get_effect_status), 9 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, download), 10 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, unload), 11 * ptr_size);
        assert_eq!(offset_of!(IDirectInputEffectVtbl, escape), 12 * ptr_size);
    }

    #[test]
    fn test_dipropcal_header_and_registry_binary_layout() {
        let cal = DipropCal::new(0x0000_0102, DIPH_BYID, -32768, 0, 32767);
        assert_eq!(cal.diph.dw_size, 28);
        assert_eq!(cal.diph.dw_header_size, 16);
        assert_eq!(cal.diph.dw_obj, 0x0000_0102);
        assert_eq!(cal.diph.dw_how, DIPH_BYID);

        let reg_bytes = cal.to_registry_bytes();
        assert_eq!(reg_bytes.len(), 12);
        assert_eq!(i32::from_le_bytes(reg_bytes[0..4].try_into().unwrap()), -32768);
        assert_eq!(i32::from_le_bytes(reg_bytes[4..8].try_into().unwrap()), 0);
        assert_eq!(i32::from_le_bytes(reg_bytes[8..12].try_into().unwrap()), 32767);
    }

    #[test]
    fn test_makediprop_pseudo_guid_constants() {
        assert_eq!(makediprop(DIPROP_BUFFERSIZE) as usize, 1);
        assert_eq!(makediprop(DIPROP_AXISMODE) as usize, 2);
        assert_eq!(makediprop(DIPROP_GRANULARITY) as usize, 3);
        assert_eq!(makediprop(DIPROP_RANGE) as usize, 4);
        assert_eq!(makediprop(DIPROP_DEADZONE) as usize, 5);
        assert_eq!(makediprop(DIPROP_SATURATION) as usize, 6);
        assert_eq!(makediprop(DIPROP_FFGAIN) as usize, 7);
        assert_eq!(makediprop(DIPROP_FFLOAD) as usize, 8);
        assert_eq!(makediprop(DIPROP_AUTOCENTER) as usize, 9);
        assert_eq!(makediprop(DIPROP_CALIBRATIONMODE) as usize, 10);
        assert_eq!(makediprop(DIPROP_CALIBRATION) as usize, 11);
        assert_eq!(makediprop(DIPROP_GUIDANDPATH) as usize, 12);
        assert_eq!(makediprop(DIPROP_PHYSICALRANGE) as usize, 18);
        assert_eq!(makediprop(DIPROP_LOGICALRANGE) as usize, 19);
    }

    #[test]
    fn test_guid_vid_pid_extraction() {
        let product_guid = Guid::new(
            0xC24F_046D,
            0x0000,
            0x0000,
            [0x00, 0x00, 0x50, 0x49, 0x44, 0x56, 0x49, 0x44],
        );
        let (vid, pid) = product_guid.vid_pid();
        assert_eq!(vid, 0x046D);
        assert_eq!(pid, 0xC24F);
    }

    #[test]
    fn test_vtbl_slot_indices_match_phase1_x86_offsets() {
        let ptr_size = size_of::<usize>();
        assert_eq!(offset_of!(IDirectInput8WVtbl, create_device), 3 * ptr_size);
        assert_eq!(offset_of!(IDirectInput8WVtbl, enum_devices), 4 * ptr_size);

        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, get_capabilities), 3 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, enum_objects), 4 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, get_property), 5 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, set_property), 6 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, acquire), 7 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, unacquire), 8 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, get_device_state), 9 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, set_data_format), 11 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, set_cooperative_level), 13 * ptr_size);
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, create_effect), 18 * ptr_size);
        assert_eq!(
            offset_of!(IDirectInputDevice8WVtbl, send_force_feedback_command),
            22 * ptr_size
        );
        assert_eq!(offset_of!(IDirectInputDevice8WVtbl, poll), 25 * ptr_size);
    }

    #[test]
    fn test_winmm_timer_and_kernel32_priority_ffi() {
        unsafe {
            let begin_rc = timeBeginPeriod(1);
            assert_eq!(begin_rc, TIMERR_NOERROR, "timeBeginPeriod(1) must succeed");

            let proc = GetCurrentProcess();
            assert!(!proc.is_null());
            let orig_prio = GetPriorityClass(proc);
            assert_ne!(orig_prio, 0, "GetPriorityClass must return non-zero class");

            let set_rc = SetPriorityClass(proc, HIGH_PRIORITY_CLASS);
            assert_ne!(set_rc, 0, "SetPriorityClass(HIGH_PRIORITY_CLASS) must succeed");
            let _ = SetPriorityClass(proc, orig_prio);

            let end_rc = timeEndPeriod(1);
            assert_eq!(end_rc, TIMERR_NOERROR, "timeEndPeriod(1) must succeed");
        }
    }
}
