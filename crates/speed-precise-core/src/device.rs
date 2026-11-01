//! DirectInput8 controller enumeration, axis capability discovery, and DIView's
//! dual-handle (`pCookedDevice` + `pRawDevice`) simultaneous polling architecture.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr;

use crate::calibration::{
    default_backup_dir, evaluate_calibrated_output_pct, get_axis_calibration, get_axis_range,
    get_deadzone, get_saturation, persist_calibration_backup_in_dir, set_axis_calibration,
    set_calibration_mode, set_deadzone, set_saturation, AxisCalibration,
};
use crate::dinput_ffi::{
    axis_guid_name, didft_get_instance, DirectInput8Create, GetModuleHandleW, DiDataFormat,
    DiDevCaps, DiDeviceInstanceW, DiDeviceObjectInstanceW, DiObjectDataFormat, Guid,
    IDirectInput8W, IDirectInputDevice8W, DI8DEVCLASS_GAMECTRL, DIDC_FORCEFEEDBACK, DIDFT_AXIS,
    DIDF_ABSAXIS, DIEDFL_ATTACHEDONLY, DIENUM_CONTINUE, DIPROP_LOGICALRANGE,
    DIPROP_PHYSICALRANGE, DIPROP_RANGE, DIRECTINPUT_VERSION, DISCL_BACKGROUND, DISCL_NONEXCLUSIVE,
    IID_IDIRECTINPUT8W,
};
use crate::error::{check_hresult, CoreError, CoreResult};

/// Static metadata and current calibration properties for a single controller axis.
#[derive(Debug, Clone)]
pub struct AxisInfo {
    /// 0-based sequential index among discovered axes on this controller.
    pub axis_index: usize,
    /// Full DirectInput `dwType` identifier (used with `DIPH_BYID = 2`).
    pub dw_type: u32,
    /// 16-bit instance ID extracted via `DIDFT_GETINSTANCE(dwType)`.
    pub instance_id: u16,
    /// Localized axis name reported by the USB HID / DirectInput driver.
    pub name: String,
    /// Standard DirectInput axis type GUID (`GUID_XAxis`, `GUID_YAxis`, etc.).
    pub guid_type: Guid,
    /// Human-readable standard axis label (`"X Axis"`, `"Rz Axis"`, etc.).
    pub standard_label: &'static str,
    /// Hardware logical encoder bounds (`DIPROP_LOGICALRANGE = 19`).
    pub logical_range: (i32, i32),
    /// Physical range reported by HID descriptor (`DIPROP_PHYSICALRANGE = 18`).
    pub physical_range: (i32, i32),
    /// Output range configured on the cooked handle (`DIPROP_RANGE = 4`).
    pub output_range: (i32, i32),
    /// Current 3-point calibration `[lMin, lCenter, lMax]` (`DIPROP_CALIBRATION = 11 / 0x0B`).
    pub calibration: AxisCalibration,
    /// Current deadzone (`0..=10000`, `DIPROP_DEADZONE = 5`).
    pub deadzone: u32,
    /// Current saturation (`0..=10000`, `DIPROP_SATURATION = 6`).
    pub saturation: u32,
}

/// Metadata and discovered axes for a connected DirectInput8 controller.
#[derive(Debug, Clone)]
pub struct ControllerDeviceInfo {
    pub index: usize,
    pub instance_name: String,
    pub product_name: String,
    pub guid_instance: Guid,
    pub guid_product: Guid,
    pub vid: u16,
    pub pid: u16,
    pub has_ffb: bool,
    pub num_buttons: u32,
    pub num_povs: u32,
    pub axes: Vec<AxisInfo>,
}

/// Simultaneous Raw + Cooked telemetry snapshot for a single axis at a given poll tick.
#[derive(Debug, Clone)]
pub struct AxisTelemetrySnapshot {
    pub axis_index: usize,
    pub dw_type: u32,
    pub instance_id: u16,
    pub name: String,
    pub standard_label: &'static str,
    pub raw_value: i32,
    pub cooked_value: i32,
    pub logical_range: (i32, i32),
    pub output_range: (i32, i32),
    pub calibration: AxisCalibration,
    pub deadzone: u32,
    pub saturation: u32,
    /// Cooked output normalized to `0.0..=100.0` across `output_range`.
    pub cooked_percent: f64,
}

/// Summary returned after applying and verifying a live calibration update.
#[derive(Debug, Clone)]
pub struct LiveCalibrationOutcome {
    pub device_name: String,
    pub vid: u16,
    pub pid: u16,
    pub axis_index: usize,
    pub axis_dw_type: u32,
    pub axis_name: String,
    pub backup_path: PathBuf,
    pub previous_calibration: AxisCalibration,
    pub readback_calibration: AxisCalibration,
    pub readback_deadzone: u32,
    pub readback_saturation: u32,
}

/// Auto-detected DirectInput8 `dw_type` assignments for the 4 primary sim-racing cockpit controls:
/// Horizontal Steering (`steering`) + 3 Vertical Pedals (`clutch` Left, `brake` Center, `throttle` Right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CockpitAxisMapping {
    pub steering: Option<u32>,
    pub clutch: Option<u32>,
    pub brake: Option<u32>,
    pub throttle: Option<u32>,
}

impl CockpitAxisMapping {
    /// Automatically maps discovered controller axes to Steering, Clutch, Brake, and Accelerator,
    /// supporting Logitech G25/G27/G29/G920/G923, Thrustmaster, Fanatec, Moza, and generic HID pedals.
    pub fn detect(device: &ControllerDeviceInfo) -> Self {
        let mut mapping = Self::default();
        let mut used: Vec<u32> = Vec::with_capacity(4);

        let mut claim = |slot: &mut Option<u32>, pred: &dyn Fn(&AxisInfo) -> bool| {
            if slot.is_some() {
                return;
            }
            if let Some(ax) = device
                .axes
                .iter()
                .find(|a| !used.contains(&a.dw_type) && pred(a))
            {
                *slot = Some(ax.dw_type);
                used.push(ax.dw_type);
            }
        };

        // Pass 1: Explicit driver / HID axis name keywords (case-insensitive)
        claim(&mut mapping.steering, &|a| {
            let n = a.name.to_ascii_lowercase();
            n.contains("wheel")
                || n.contains("steer")
                || n.contains("volante")
                || n.contains("dire")
        });
        claim(&mut mapping.clutch, &|a| {
            let n = a.name.to_ascii_lowercase();
            n.contains("clutch") || n.contains("embreag")
        });
        claim(&mut mapping.brake, &|a| {
            let n = a.name.to_ascii_lowercase();
            n.contains("brake") || n.contains("freio")
        });
        claim(&mut mapping.throttle, &|a| {
            let n = a.name.to_ascii_lowercase();
            n.contains("accel")
                || n.contains("thrott")
                || n.contains("aceler")
                || n == "gas"
        });

        // Pass 2: Standard DirectInput GUID axis conventions (`X Axis`, `Y Axis`, `Rz Axis`, `Slider`, `Z Axis`)
        let likely_has_wheel = device.has_ffb || device.axes.len() >= 4 || device.axes.len() == 1;
        if likely_has_wheel {
            claim(&mut mapping.steering, &|a| a.standard_label == "X Axis");
        }
        claim(&mut mapping.throttle, &|a| a.standard_label == "Y Axis");
        claim(&mut mapping.throttle, &|a| a.standard_label == "Z Axis");
        claim(&mut mapping.brake, &|a| a.standard_label == "Rz Axis");
        claim(&mut mapping.brake, &|a| a.standard_label == "Ry Axis");
        claim(&mut mapping.brake, &|a| a.standard_label == "Z Axis");
        claim(&mut mapping.clutch, &|a| a.standard_label == "Slider");
        claim(&mut mapping.clutch, &|a| a.standard_label == "Dial");
        claim(&mut mapping.clutch, &|a| a.standard_label == "Rx Axis");

        // Pass 3: Standalone 3-pedal USB box fallback (e.g. X=Clutch/Throttle, Y=Brake, Z=Throttle/Clutch)
        if !likely_has_wheel && device.axes.len() == 3 {
            claim(&mut mapping.clutch, &|_| true);
            claim(&mut mapping.brake, &|_| true);
            claim(&mut mapping.throttle, &|_| true);
        } else {
            if likely_has_wheel {
                claim(&mut mapping.steering, &|_| true);
            }
            claim(&mut mapping.throttle, &|_| true);
            claim(&mut mapping.brake, &|_| true);
            claim(&mut mapping.clutch, &|_| true);
        }

        mapping
    }
}

/// Returns `true` when a pedal axis uses inverted active-low potentiometers by default
/// (resting = `logical_max`, fully pressed = `logical_min`), such as all Logitech Driving Force /
/// G25 / G27 / G29 / G920 / G923 pedal sets (`VID == 0x046D`) or axes whose calibrated `max`
/// stays anchored at `logical_max` while `min` is raised above `logical_min`.
pub fn default_pedal_inverted(device: &ControllerDeviceInfo, axis: &AxisInfo) -> bool {
    if device.vid == 0x046D {
        return true;
    }
    let (l_min, l_max) = axis.logical_range;
    let span = (l_max as i64) - (l_min as i64);
    if span > 10 && axis.calibration.max == l_max && (axis.calibration.min as i64) > (l_min as i64) + (span / 20) {
        return true;
    }
    false
}

/// RAII wrapper around `IDirectInput8W`.
pub struct DirectInputContext {
    ptr: *mut IDirectInput8W,
}

impl DirectInputContext {
    /// Initializes a DirectInput8 COM interface (`DirectInput8Create` with `0x0800`).
    pub fn new() -> CoreResult<Self> {
        unsafe {
            let hinst = GetModuleHandleW(ptr::null());
            let mut raw_ptr: *mut c_void = ptr::null_mut();
            let hr = DirectInput8Create(
                hinst,
                DIRECTINPUT_VERSION,
                &IID_IDIRECTINPUT8W,
                &mut raw_ptr,
                ptr::null_mut(),
            );
            check_hresult("DirectInput8Create", hr)?;
            Ok(Self {
                ptr: raw_ptr as *mut IDirectInput8W,
            })
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut IDirectInput8W {
        self.ptr
    }

    /// Enumerates all attached game controllers (`DI8DEVCLASS_GAMECTRL = 4`, `DIEDFL_ATTACHEDONLY = 1`)
    /// along with their axes and live calibration parameters.
    pub fn enumerate_controllers(&self) -> CoreResult<Vec<ControllerDeviceInfo>> {
        unsafe {
            let mut raw_instances: Vec<DiDeviceInstanceW> = Vec::new();
            let hr = ((*(*self.ptr).lp_vtbl).enum_devices)(
                self.ptr,
                DI8DEVCLASS_GAMECTRL,
                enum_devices_callback,
                (&mut raw_instances as *mut Vec<DiDeviceInstanceW>).cast::<c_void>(),
                DIEDFL_ATTACHEDONLY,
            );
            check_hresult("IDirectInput8W::EnumDevices", hr)?;

            let mut controllers = Vec::with_capacity(raw_instances.len());
            for (index, inst) in raw_instances.iter().enumerate() {
                if let Ok(info) = self.inspect_device_instance(index, inst) {
                    controllers.push(info);
                }
            }
            Ok(controllers)
        }
    }

    /// Opens a DIView-style dual handle (`pCookedDevice` + `pRawDevice`) for the controller at `index`.
    pub fn open_dual_handle(&self, index: usize) -> CoreResult<DualDeviceHandle> {
        let controllers = self.enumerate_controllers()?;
        let total = controllers.len();
        let info = controllers
            .into_iter()
            .nth(index)
            .ok_or(CoreError::DeviceIndexOutOfRange { index, total })?;
        DualDeviceHandle::open(self, info)
    }

    unsafe fn create_device_handle(
        &self,
        guid_instance: &Guid,
    ) -> CoreResult<*mut IDirectInputDevice8W> {
        let mut dev_ptr: *mut IDirectInputDevice8W = ptr::null_mut();
        let hr = ((*(*self.ptr).lp_vtbl).create_device)(
            self.ptr,
            guid_instance,
            &mut dev_ptr,
            ptr::null_mut(),
        );
        check_hresult("IDirectInput8W::CreateDevice", hr)?;
        Ok(dev_ptr)
    }

    unsafe fn inspect_device_instance(
        &self,
        index: usize,
        inst: &DiDeviceInstanceW,
    ) -> CoreResult<ControllerDeviceInfo> {
        let dev_ptr = self.create_device_handle(&inst.guid_instance)?;

        let mut caps = DiDevCaps::new();
        let _ = ((*(*dev_ptr).lp_vtbl).get_capabilities)(dev_ptr, &mut caps);
        let has_ffb = (caps.dw_flags & DIDC_FORCEFEEDBACK) != 0;

        let mut raw_axes: Vec<DiDeviceObjectInstanceW> = Vec::new();
        let enum_hr = ((*(*dev_ptr).lp_vtbl).enum_objects)(
            dev_ptr,
            enum_axes_callback,
            (&mut raw_axes as *mut Vec<DiDeviceObjectInstanceW>).cast::<c_void>(),
            DIDFT_AXIS,
        );

        let mut axes = Vec::new();
        if enum_hr >= 0 {
            for (axis_index, obj) in raw_axes.iter().enumerate() {
                let dw_type = obj.dw_type;
                let instance_id = didft_get_instance(dw_type);
                let logical_range =
                    get_axis_range(dev_ptr, DIPROP_LOGICALRANGE, dw_type).unwrap_or((0, 65535));
                let physical_range =
                    get_axis_range(dev_ptr, DIPROP_PHYSICALRANGE, dw_type).unwrap_or(logical_range);
                let output_range =
                    get_axis_range(dev_ptr, DIPROP_RANGE, dw_type).unwrap_or(logical_range);
                let calibration = get_axis_calibration(dev_ptr, dw_type).unwrap_or_else(|_| {
                    AxisCalibration::default_reset(logical_range.0, logical_range.1)
                });
                let deadzone = get_deadzone(dev_ptr, dw_type).unwrap_or(0);
                let saturation = get_saturation(dev_ptr, dw_type).unwrap_or(10_000);

                axes.push(AxisInfo {
                    axis_index,
                    dw_type,
                    instance_id,
                    name: obj.name(),
                    guid_type: obj.guid_type,
                    standard_label: axis_guid_name(&obj.guid_type),
                    logical_range,
                    physical_range,
                    output_range,
                    calibration,
                    deadzone,
                    saturation,
                });
            }
        }

        let _ = ((*(*dev_ptr).lp_vtbl).release)(dev_ptr);

        let (vid, pid) = inst.guid_product.vid_pid();
        Ok(ControllerDeviceInfo {
            index,
            instance_name: inst.instance_name(),
            product_name: inst.product_name(),
            guid_instance: inst.guid_instance,
            guid_product: inst.guid_product,
            vid,
            pid,
            has_ffb,
            num_buttons: caps.dw_buttons,
            num_povs: caps.dw_povs,
            axes,
        })
    }
}

impl Drop for DirectInputContext {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                let _ = ((*(*self.ptr).lp_vtbl).release)(self.ptr);
            }
            self.ptr = ptr::null_mut();
        }
    }
}

unsafe extern "system" fn enum_devices_callback(
    lpddi: *const DiDeviceInstanceW,
    pv_ref: *mut c_void,
) -> i32 {
    if !lpddi.is_null() && !pv_ref.is_null() {
        let list = &mut *(pv_ref as *mut Vec<DiDeviceInstanceW>);
        list.push(*lpddi);
    }
    DIENUM_CONTINUE
}

unsafe extern "system" fn enum_axes_callback(
    lpddoi: *const DiDeviceObjectInstanceW,
    pv_ref: *mut c_void,
) -> i32 {
    if !lpddoi.is_null() && !pv_ref.is_null() {
        let list = &mut *(pv_ref as *mut Vec<DiDeviceObjectInstanceW>);
        list.push(*lpddoi);
    }
    DIENUM_CONTINUE
}

/// DIView-style dual-handle device controller (`pCookedDevice` + `pRawDevice`).
///
/// Replicates `DIView.exe` `FUN_00404560` and `FUN_004043d0`:
/// - `p_cooked_device`: standard calibrated handle (`DIPROPCALIBRATIONMODE_COOKED = 0`)
/// - `p_raw_device`: raw hardware encoder handle (`DIPROPCALIBRATIONMODE_RAW = 1` via `DIPROP_CALIBRATIONMODE = 0x0A`)
/// - Both handles use `DISCL_NONEXCLUSIVE | DISCL_BACKGROUND` (`0x0A = 10`) and a dynamic
///   `DIDATAFORMAT` mapping each discovered axis `i` to byte offset `i * 4`.
pub struct DualDeviceHandle {
    pub info: ControllerDeviceInfo,
    p_cooked_device: *mut IDirectInputDevice8W,
    p_raw_device: *mut IDirectInputDevice8W,
    _object_formats: Vec<DiObjectDataFormat>,
    raw_state_buf: Vec<i32>,
    cooked_state_buf: Vec<i32>,
    cooked_awaiting_hid_sync: Vec<bool>,
    sync_ref_raw: Vec<i32>,
}

impl DualDeviceHandle {
    pub fn open(ctx: &DirectInputContext, info: ControllerDeviceInfo) -> CoreResult<Self> {
        unsafe {
            let p_cooked_device = ctx.create_device_handle(&info.guid_instance)?;
            let p_raw_device = match ctx.create_device_handle(&info.guid_instance) {
                Ok(ptr) => ptr,
                Err(e) => {
                    let _ = ((*(*p_cooked_device).lp_vtbl).release)(p_cooked_device);
                    return Err(e);
                }
            };

            // Build dynamic DIDATAFORMAT matching DIView FUN_00404560:
            // Each discovered axis `i` is placed at byte offset `i * 4` using its exact `dw_type`.
            let num_axes = info.axes.len();
            let mut object_formats = Vec::with_capacity(num_axes.max(1));
            for (i, axis) in info.axes.iter().enumerate() {
                object_formats.push(DiObjectDataFormat {
                    pguid: ptr::null(),
                    dw_ofs: (i * 4) as u32,
                    dw_type: axis.dw_type,
                    dw_flags: 0,
                });
            }

            let data_size_i32 = num_axes.max(1);
            let data_format = DiDataFormat {
                dw_size: size_of::<DiDataFormat>() as u32,
                dw_obj_size: size_of::<DiObjectDataFormat>() as u32,
                dw_flags: DIDF_ABSAXIS,
                dw_data_size: (data_size_i32 * 4) as u32,
                dw_num_objs: object_formats.len() as u32,
                rgodf: if object_formats.is_empty() {
                    ptr::null()
                } else {
                    object_formats.as_ptr()
                },
            };

            let coop_flags = DISCL_NONEXCLUSIVE | DISCL_BACKGROUND; // 0x02 | 0x08 = 10 (0x0A)

            if num_axes > 0 {
                let _ = ((*(*p_cooked_device).lp_vtbl).set_data_format)(
                    p_cooked_device,
                    &data_format,
                );
                let _ =
                    ((*(*p_raw_device).lp_vtbl).set_data_format)(p_raw_device, &data_format);
            }

            let _ = ((*(*p_cooked_device).lp_vtbl).set_cooperative_level)(
                p_cooked_device,
                ptr::null_mut(),
                coop_flags,
            );
            let _ = ((*(*p_raw_device).lp_vtbl).set_cooperative_level)(
                p_raw_device,
                ptr::null_mut(),
                coop_flags,
            );

            // Set DIPROP_CALIBRATIONMODE (0x0A) = DIPROPCALIBRATIONMODE_RAW (1) on p_raw_device for every axis
            for axis in &info.axes {
                let _ = set_calibration_mode(p_raw_device, axis.dw_type, true);
            }

            let _ = ((*(*p_cooked_device).lp_vtbl).acquire)(p_cooked_device);
            let _ = ((*(*p_raw_device).lp_vtbl).acquire)(p_raw_device);

            Ok(Self {
                info,
                p_cooked_device,
                p_raw_device,
                _object_formats: object_formats,
                raw_state_buf: vec![0i32; data_size_i32],
                cooked_state_buf: vec![0i32; data_size_i32],
                cooked_awaiting_hid_sync: vec![false; data_size_i32],
                sync_ref_raw: vec![0i32; data_size_i32],
            })
        }
    }

    /// Resolves a user-supplied `--axis <id>` selector to a reference in `self.info.axes`.
    /// Matches in priority order:
    /// 1. Exact `dw_type` (e.g. `0x00000102` / `258`),
    /// 2. 0-based `axis_index` (`0..axes.len()`),
    /// 3. 16-bit `instance_id` (`DIDFT_GETINSTANCE(dwType)`).
    pub fn find_axis_index(&self, axis_selector: u32) -> CoreResult<usize> {
        if let Some(pos) = self
            .info
            .axes
            .iter()
            .position(|a| a.dw_type == axis_selector)
        {
            return Ok(pos);
        }
        if let Some(pos) = self
            .info
            .axes
            .iter()
            .position(|a| a.axis_index as u32 == axis_selector)
        {
            return Ok(pos);
        }
        if let Some(pos) = self
            .info
            .axes
            .iter()
            .position(|a| a.instance_id as u32 == axis_selector)
        {
            return Ok(pos);
        }
        Err(CoreError::AxisNotFound {
            axis_id: axis_selector,
            device_name: self.info.instance_name.clone(),
        })
    }

    /// Polls both `pRawDevice` and `pCookedDevice` simultaneously (`Poll` + `GetDeviceState`)
    /// and returns per-axis Raw and Calibrated snapshots.
    pub fn poll_telemetry(&mut self) -> CoreResult<Vec<AxisTelemetrySnapshot>> {
        if self.info.axes.is_empty() {
            return Ok(Vec::new());
        }

        let byte_len = (self.raw_state_buf.len() * size_of::<i32>()) as u32;
        unsafe {
            poll_and_get_state(
                self.p_raw_device,
                byte_len,
                self.raw_state_buf.as_mut_ptr().cast::<c_void>(),
            )?;
            poll_and_get_state(
                self.p_cooked_device,
                byte_len,
                self.cooked_state_buf.as_mut_ptr().cast::<c_void>(),
            )?;
        }

        let mut snapshots = Vec::with_capacity(self.info.axes.len());
        for (i, axis) in self.info.axes.iter().enumerate() {
            let raw_value = self.raw_state_buf[i];
            let mut cooked_value = self.cooked_state_buf[i];
            let (out_min, out_max) = axis.output_range;
            let span = (out_max as f64) - (out_min as f64);
            let out_mid = (((out_min as i64) + (out_max as i64)) / 2) as i32;

            if self.cooked_awaiting_hid_sync[i] {
                let raw_moved_to_center = raw_value != self.sync_ref_raw[i]
                    && (raw_value - axis.calibration.center).abs() <= 1;
                if cooked_value != out_mid || raw_moved_to_center {
                    self.cooked_awaiting_hid_sync[i] = false;
                }
            }

            let cooked_percent = if self.cooked_awaiting_hid_sync[i] {
                let pct = evaluate_calibrated_output_pct(
                    raw_value,
                    axis.calibration,
                    axis.deadzone,
                    axis.saturation,
                );
                if span.abs() > f64::EPSILON {
                    cooked_value = ((out_min as f64) + (pct / 100.0) * span).round() as i32;
                }
                pct
            } else if span.abs() > f64::EPSILON {
                (((cooked_value as f64) - (out_min as f64)) / span * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };

            snapshots.push(AxisTelemetrySnapshot {
                axis_index: axis.axis_index,
                dw_type: axis.dw_type,
                instance_id: axis.instance_id,
                name: axis.name.clone(),
                standard_label: axis.standard_label,
                raw_value,
                cooked_value,
                logical_range: axis.logical_range,
                output_range: axis.output_range,
                calibration: axis.calibration,
                deadzone: axis.deadzone,
                saturation: axis.saturation,
                cooked_percent,
            });
        }

        Ok(snapshots)
    }

    /// Applies live calibration `[min, center, max]` (plus optional `deadzone` and `saturation`)
    /// via `SetProperty(DIPROP_CALIBRATION /* 0x0B */)` after persisting a safety backup to
    /// `%APPDATA%\SpeedPrecise\backups\`, then verifies the round-trip readback from the driver.
    pub fn apply_live_calibration(
        &mut self,
        axis_selector: u32,
        min: i32,
        center: i32,
        max: i32,
        deadzone: Option<u32>,
        saturation: Option<u32>,
    ) -> CoreResult<LiveCalibrationOutcome> {
        let new_cal = AxisCalibration::new(min, center, max)?;
        let pos = self.find_axis_index(axis_selector)?;
        let axis_dw_type = self.info.axes[pos].dw_type;
        let axis_name = self.info.axes[pos].name.clone();
        let prev_cal = self.info.axes[pos].calibration;
        let prev_dz = Some(self.info.axes[pos].deadzone);
        let prev_sat = Some(self.info.axes[pos].saturation);

        // Rule 2 (AGENTS.md): Persist calibration backup before writing changes
        let backup_dir = default_backup_dir();
        let backup_path = persist_calibration_backup_in_dir(
            &backup_dir,
            self.info.vid,
            self.info.pid,
            &self.info.instance_name,
            axis_dw_type,
            prev_cal,
            prev_dz,
            prev_sat,
        )?;

        unsafe {
            // Unacquire before mutating axis calibration/deadzone/saturation properties so
            // dinput8.dll rebuilds the axis ramp cleanly, then re-acquire and poll.
            let _ = ((*(*self.p_cooked_device).lp_vtbl).unacquire)(self.p_cooked_device);

            set_axis_calibration(self.p_cooked_device, axis_dw_type, new_cal)?;
            if let Some(dz) = deadzone {
                if dz != self.info.axes[pos].deadzone {
                    set_deadzone(self.p_cooked_device, axis_dw_type, dz)?;
                }
            }
            if let Some(sat) = saturation {
                if sat != self.info.axes[pos].saturation {
                    set_saturation(self.p_cooked_device, axis_dw_type, sat)?;
                }
            }

            let readback_calibration =
                get_axis_calibration(self.p_cooked_device, axis_dw_type)?;
            let readback_deadzone =
                get_deadzone(self.p_cooked_device, axis_dw_type).unwrap_or(0);
            let readback_saturation =
                get_saturation(self.p_cooked_device, axis_dw_type).unwrap_or(10_000);

            self.info.axes[pos].calibration = readback_calibration;
            self.info.axes[pos].deadzone = readback_deadzone;
            self.info.axes[pos].saturation = readback_saturation;

            let _ = ((*(*self.p_cooked_device).lp_vtbl).acquire)(self.p_cooked_device);
            let _ = ((*(*self.p_cooked_device).lp_vtbl).poll)(self.p_cooked_device);

            // dinput8.dll resets `p_cooked_device`'s internal axis value to `(out_min + out_max) / 2`
            // (`32767` = `50.0%`) after `SetProperty` / `Acquire` until the next USB HID report arrives.
            if pos < self.cooked_awaiting_hid_sync.len() {
                self.cooked_awaiting_hid_sync[pos] = true;
                self.sync_ref_raw[pos] = self.raw_state_buf[pos];
            }

            Ok(LiveCalibrationOutcome {
                device_name: self.info.instance_name.clone(),
                vid: self.info.vid,
                pid: self.info.pid,
                axis_index: pos,
                axis_dw_type,
                axis_name,
                backup_path,
                previous_calibration: prev_cal,
                readback_calibration,
                readback_deadzone,
                readback_saturation,
            })
        }
    }
}

unsafe fn poll_and_get_state(
    device: *mut IDirectInputDevice8W,
    cb_data: u32,
    lpv_data: *mut c_void,
) -> CoreResult<()> {
    let poll_hr = ((*(*device).lp_vtbl).poll)(device);
    if poll_hr < 0 {
        let _ = ((*(*device).lp_vtbl).acquire)(device);
        let _ = ((*(*device).lp_vtbl).poll)(device);
    }

    let mut state_hr = ((*(*device).lp_vtbl).get_device_state)(device, cb_data, lpv_data);
    if state_hr < 0 {
        // Re-acquire once on DIERR_INPUTLOST / DIERR_NOTACQUIRED (matches DIView FUN_004043d0)
        let _ = ((*(*device).lp_vtbl).acquire)(device);
        let _ = ((*(*device).lp_vtbl).poll)(device);
        state_hr = ((*(*device).lp_vtbl).get_device_state)(device, cb_data, lpv_data);
    }
    check_hresult("IDirectInputDevice8::GetDeviceState", state_hr)
}

impl Drop for DualDeviceHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.p_cooked_device.is_null() {
                let _ = ((*(*self.p_cooked_device).lp_vtbl).unacquire)(self.p_cooked_device);
                let _ = ((*(*self.p_cooked_device).lp_vtbl).release)(self.p_cooked_device);
                self.p_cooked_device = ptr::null_mut();
            }
            if !self.p_raw_device.is_null() {
                let _ = ((*(*self.p_raw_device).lp_vtbl).unacquire)(self.p_raw_device);
                let _ = ((*(*self.p_raw_device).lp_vtbl).release)(self.p_raw_device);
                self.p_raw_device = ptr::null_mut();
            }
        }
    }
}

/// Convenience helper to construct a zero-initialized `DiDeviceInstanceW` for unit testing.
pub fn empty_device_instance_w() -> DiDeviceInstanceW {
    let mut inst: DiDeviceInstanceW = unsafe { zeroed() };
    inst.dw_size = size_of::<DiDeviceInstanceW>() as u32;
    inst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directinput8_context_init_and_enumerate() {
        let ctx = DirectInputContext::new().expect("DirectInput8Create should succeed on Windows");
        assert!(!ctx.as_ptr().is_null());
        let controllers = ctx
            .enumerate_controllers()
            .expect("EnumDevices should succeed even with 0 attached controllers");
        let _ = controllers.len();
    }

    #[test]
    fn test_empty_device_instance_w_size() {
        let inst = empty_device_instance_w();
        assert_eq!(inst.dw_size, 1100);
    }

    #[test]
    fn test_cockpit_axis_mapping_g920_and_generic() {
        use crate::dinput_ffi::{GUID_RZAXIS, GUID_SLIDER, GUID_XAXIS, GUID_YAXIS};

        let mk_axis = |idx: usize, dw_type: u32, name: &str, guid: Guid, label: &'static str, max: i32| AxisInfo {
            axis_index: idx,
            dw_type,
            instance_id: idx as u16,
            name: name.to_string(),
            guid_type: guid,
            standard_label: label,
            logical_range: (0, max),
            physical_range: (0, max),
            output_range: (0, 65535),
            calibration: AxisCalibration::default_reset(0, max),
            deadzone: 0,
            saturation: 10_000,
        };

        let g920 = ControllerDeviceInfo {
            index: 0,
            instance_name: "Logitech G HUB G920 Driving Force Racing Wheel USB".to_string(),
            product_name: "Logitech G HUB G920 Driving Force Racing Wheel USB".to_string(),
            guid_instance: GUID_XAXIS,
            guid_product: GUID_XAXIS,
            vid: 0x046D,
            pid: 0xC262,
            has_ffb: true,
            num_buttons: 18,
            num_povs: 1,
            axes: vec![
                mk_axis(0, 0x0100_0002, "Wheel axis", GUID_XAXIS, "X Axis", 65535),
                mk_axis(1, 0x0000_0602, "Clutch", GUID_SLIDER, "Slider", 255),
                mk_axis(2, 0x0000_0502, "Brake", GUID_RZAXIS, "Rz Axis", 255),
                mk_axis(3, 0x0000_0202, "Accelerator", GUID_YAXIS, "Y Axis", 255),
            ],
        };

        let mapped = CockpitAxisMapping::detect(&g920);
        assert_eq!(mapped.steering, Some(0x0100_0002));
        assert_eq!(mapped.clutch, Some(0x0000_0602));
        assert_eq!(mapped.brake, Some(0x0000_0502));
        assert_eq!(mapped.throttle, Some(0x0000_0202));
        assert!(default_pedal_inverted(&g920, &g920.axes[2]));
    }
}

