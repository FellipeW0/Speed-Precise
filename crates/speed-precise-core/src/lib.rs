//! `speed-precise-core`: Core DirectInput8 FFI bindings, DIView dual-handle (`pCookedDevice` + `pRawDevice`)
//! telemetry engine, live hardware calibration (`DIPROP_CALIBRATION = 11 / 0x0B`),
//! WheelCheck `Step Log 2` Force Feedback state machine (`ffb_engine`),
//! LUT mathematical generator (`lut_math`), and Assetto Corsa profile injector (`sim_inject`).

pub mod calibration;
pub mod device;
pub mod dinput_ffi;
pub mod error;
pub mod ffb_engine;
pub mod lut_math;
pub mod sim_inject;

pub use calibration::{
    compute_pedal_press_percentages, compute_smart_pedal_calibration, compute_steering_angle_deg,
    default_backup_dir, evaluate_calibrated_output_pct, get_axis_calibration, get_axis_range,
    get_deadzone, get_saturation, infer_pedal_sliders_from_calibration,
    persist_calibration_backup_in_dir, set_axis_calibration, set_axis_range, set_calibration_mode,
    set_deadzone, set_saturation, AxisCalibration, CalibrationBackupRecord,
};
pub use device::{
    default_pedal_inverted, AxisInfo, AxisTelemetrySnapshot, CockpitAxisMapping,
    ControllerDeviceInfo, DirectInputContext, DualDeviceHandle, LiveCalibrationOutcome,
};
pub use error::{check_hresult, CoreError, CoreResult};
pub use ffb_engine::{
    compute_delta_x_deg, compute_step_force, format_step_log2_csv, normalize_raw_to_10000,
    run_step_log2_with_backend, write_step_log2_csv, FfbHardwareBackend, FfbSession,
    PidCenteringController, StepLog2Config, StepLog2Progress, StepLog2Sample, StepLog2State,
    VirtualWheelSimulator, DEFAULT_STEP_LOG2_DURATION_MS, DEFAULT_STEP_LOG2_MAX_ANGLE_DEG,
    DEFAULT_STEP_LOG2_MAX_FORCE, DEFAULT_STEP_LOG2_SETTLE_MS, DEFAULT_STEP_LOG2_STEPS,
    ENDSTOP_SAFETY_LIMIT_NORM,
};
pub use lut_math::{
    generate_inverse_lut, parse_wheelcheck_csv, read_wheelcheck_csv_file, render_ascii_lut_chart,
    round_3, write_lut_file, LutConfig, LutPoint, LutResult, StepLogSample,
    DEFAULT_DEADZONE_THRESHOLD_RATIO, DEFAULT_LUT_STEPS, DEFAULT_SMOOTHING_WINDOW,
};
pub use sim_inject::{
    default_assetto_corsa_cfg_dir, default_beamng_inputmaps_dir, inject_assetto_corsa_lut,
    inject_beamng_lut, update_ff_post_process_ini, AssettoCorsaInjectionOutcome,
    BeamNgInjectionOutcome,
};
