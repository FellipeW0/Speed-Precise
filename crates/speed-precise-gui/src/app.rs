//! Clean 2-Screen Sim Racing Dashboard (`app.rs`):
//! - **Top Header Bar:** Clean App Title, 2-Screen Navigation (`Volante e Pedais` | `Gerador de LUT`),
//!   Consumer Controller `ComboBox`, and `[Atualizar USB]` (no permanent Emergency Stop clutter).
//! - **Screen 1 (`Volante e Pedais`):**
//!   - Top Horizontal Center-Zero Steering Gauge (`-450° .. 0° .. +450°`, adapting to `540°`/`900°`/`1080°`).
//!   - Bottom 3 Vertical Pedal Columns (Left-to-Right: **1. Embreagem**, **2. Freio**, **3. Acelerador**)
//!     with dual vertical bars (`Curso Físico` + `Saída no Jogo 0%..100%`) and smart `[Min, Center, Max]`
//!     recalculation from `Curso Máximo / Saturação (10%..100%)` and `Zona Morta Inicial (0%..30%)`.
//! - **Screen 2 (`Gerador de LUT — Force Feedback`):**
//!   - Unified 1-click Auto-LUT pipeline (`Passos de Leitura`, `Ângulo do Volante`, `[Iniciar Teste e Gerar LUT]`).
//!   - Contextual `[Parar Teste Agora (ESPAÇO / ESC)]` button shown **ONLY** while `ffb_running == true`.
//!   - Single clean 3-curve LUT Chart strictly locked inside `[0.0, 1.0] × [0.0, 1.0]` with `[Resetar Zoom]`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use egui_plot::{Legend, Line, LineStyle, Plot, PlotBounds, PlotPoints};
use speed_precise_core::{
    compute_pedal_press_percentages, compute_smart_pedal_calibration, compute_steering_angle_deg,
    default_assetto_corsa_cfg_dir, default_pedal_inverted, evaluate_calibrated_output_pct,
    generate_inverse_lut, infer_pedal_sliders_from_calibration, inject_assetto_corsa_lut,
    read_wheelcheck_csv_file, write_lut_file, AxisInfo, AxisTelemetrySnapshot, CockpitAxisMapping,
    LutConfig, LutResult, StepLogSample, DEFAULT_DEADZONE_THRESHOLD_RATIO, DEFAULT_LUT_STEPS,
    DEFAULT_SMOOTHING_WINDOW, DEFAULT_STEP_LOG2_DURATION_MS, DEFAULT_STEP_LOG2_MAX_ANGLE_DEG,
    DEFAULT_STEP_LOG2_MAX_FORCE, DEFAULT_STEP_LOG2_STEPS,
};

use crate::worker::{DeviceInfo, GuiCommand, HardwareWorker, WorkerConfig, WorkerEvent};

// ============================================================================
// Neutral Black & Dark-Gray Sim-Racing Cockpit Color Palette
// (Zero blue cast across surfaces/controls; comfortable contrast & legibility)
// ============================================================================
const COLOR_BG_ROOT: Color32 = Color32::from_rgb(20, 20, 20);
const COLOR_BG_PANEL: Color32 = Color32::from_rgb(26, 26, 26);
const COLOR_BG_CARD: Color32 = Color32::from_rgb(34, 34, 34);
const COLOR_BG_TRACK: Color32 = Color32::from_rgb(16, 16, 16);
const COLOR_BORDER_SUBTLE: Color32 = Color32::from_rgb(62, 62, 62);

const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(242, 242, 242);
const COLOR_TEXT_SECONDARY: Color32 = Color32::from_rgb(212, 212, 212);
const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(163, 163, 163);

const COLOR_ACCENT_SURFACE: Color32 = Color32::from_rgb(68, 68, 68);
const COLOR_ACCENT_BORDER: Color32 = Color32::from_rgb(148, 148, 148);
const COLOR_STEERING_FILL: Color32 = Color32::from_rgb(220, 220, 220);
const COLOR_CLUTCH_CYAN: Color32 = Color32::from_rgb(0, 184, 217);
const COLOR_BRAKE_RED: Color32 = Color32::from_rgb(235, 77, 75);
const COLOR_THROTTLE_GREEN: Color32 = Color32::from_rgb(38, 194, 129);
const COLOR_PHYSICAL_RAW: Color32 = Color32::from_rgb(132, 132, 132);

const COLOR_EMERGENCY_ACTIVE: Color32 = Color32::from_rgb(225, 38, 48);
const COLOR_PLOT_IDEAL: Color32 = Color32::from_rgb(158, 158, 158);
const COLOR_PLOT_RAW: Color32 = Color32::from_rgb(235, 77, 75);
const COLOR_PLOT_LUT: Color32 = Color32::from_rgb(46, 213, 115);
const COLOR_BADGE_WARN: Color32 = Color32::from_rgb(243, 156, 18);
const COLOR_BADGE_OK: Color32 = Color32::from_rgb(39, 174, 96);

/// Minimum zoom span (`5%` of `[0.0, 1.0]`) allowed inside the bounded LUT chart.
pub const MIN_LUT_ZOOM_SPAN: f64 = 0.05;
/// Maximum zoom span (`100%` of `[0.0, 1.0]`) allowed inside the bounded LUT chart.
pub const MAX_LUT_ZOOM_SPAN: f64 = 1.0;

/// Active screen in the clean 2-screen Speed Precise dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveScreen {
    /// Screen 1: Horizontal Steering + 3 Vertical Pedals (`Embreagem`, `Freio`, `Acelerador`).
    CockpitDashboard,
    /// Screen 2: Unified 1-Click Force Feedback Test & Bounded `[0.0, 1.0]` LUT Generator.
    AutoLutStudio,
}

/// Logical role of each vertical pedal column (ordered strictly Left-to-Right in the cockpit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PedalRole {
    Clutch,
    Brake,
    Throttle,
}

impl PedalRole {
    pub const ALL_LEFT_TO_RIGHT: [Self; 3] = [Self::Clutch, Self::Brake, Self::Throttle];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Clutch => "EMBREAGEM",
            Self::Brake => "FREIO",
            Self::Throttle => "ACELERADOR",
        }
    }

    pub const fn position_hint(self) -> &'static str {
        match self {
            Self::Clutch => "Pedal Esquerdo",
            Self::Brake => "Pedal Central",
            Self::Throttle => "Pedal Direito",
        }
    }

    pub const fn accent_color(self) -> Color32 {
        match self {
            Self::Clutch => COLOR_CLUTCH_CYAN,
            Self::Brake => COLOR_BRAKE_RED,
            Self::Throttle => COLOR_THROTTLE_GREEN,
        }
    }
}

/// Consumer-friendly smart saturation & deadzone state for a single pedal axis.
#[derive(Debug, Clone)]
pub struct PedalSliderState {
    /// Physical travel required to reach `100%` in-game output (`10.0..=100.0`, default `100.0`).
    pub saturation_pct: f64,
    /// Initial resting travel ignored before output rises above `0%` (`0.0..=30.0`, default `0.0`).
    pub deadzone_pct: f64,
    /// `true` when resting foot-off value sits at `logical_max` and full press is `logical_min`
    /// (standard on Logitech G25/G27/G29/G920/G923 pedal sets).
    pub inverted: bool,
}

impl PedalSliderState {
    pub fn from_device_axis(dev: &DeviceInfo, axis: &AxisInfo) -> Self {
        let inverted = default_pedal_inverted(dev, axis);
        let (l_min, l_max) = axis.logical_range;
        let (saturation_pct, deadzone_pct) =
            infer_pedal_sliders_from_calibration(l_min, l_max, axis.calibration, inverted);
        Self {
            saturation_pct,
            deadzone_pct,
            inverted,
        }
    }
}

/// Per-controller cockpit mapping (`Volante` + `Embreagem` + `Freio` + `Acelerador`).
#[derive(Debug, Clone)]
pub struct DeviceCockpitState {
    pub mapping: CockpitAxisMapping,
    pub pedal_states: HashMap<u32, PedalSliderState>,
}

impl DeviceCockpitState {
    pub fn from_device(dev: &DeviceInfo) -> Self {
        let mapping = CockpitAxisMapping::detect(dev);
        let mut pedal_states = HashMap::with_capacity(dev.axes.len());
        for axis in &dev.axes {
            pedal_states.insert(axis.dw_type, PedalSliderState::from_device_axis(dev, axis));
        }
        Self {
            mapping,
            pedal_states,
        }
    }

    pub fn axis_for_pedal(&self, role: PedalRole) -> Option<u32> {
        match role {
            PedalRole::Clutch => self.mapping.clutch,
            PedalRole::Brake => self.mapping.brake,
            PedalRole::Throttle => self.mapping.throttle,
        }
    }

    pub fn set_axis_for_pedal(&mut self, role: PedalRole, axis_dw_type: Option<u32>) {
        match role {
            PedalRole::Clutch => self.mapping.clutch = axis_dw_type,
            PedalRole::Brake => self.mapping.brake = axis_dw_type,
            PedalRole::Throttle => self.mapping.throttle = axis_dw_type,
        }
    }
}

/// Strictly clamps 2D `egui_plot` viewport bounds inside the unit square `[0.0, 1.0] × [0.0, 1.0]`,
/// preserving the user's current zoom span (`0.05..=1.0`) when panning against any outer border.
pub fn clamp_unit_square_bounds(mut min: [f64; 2], mut max: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    for axis in 0..2 {
        if !min[axis].is_finite() || !max[axis].is_finite() {
            min[axis] = 0.0;
            max[axis] = 1.0;
            continue;
        }
        if min[axis] > max[axis] {
            std::mem::swap(&mut min[axis], &mut max[axis]);
        }

        let span = (max[axis] - min[axis]).clamp(MIN_LUT_ZOOM_SPAN, MAX_LUT_ZOOM_SPAN);
        let center = (min[axis] + max[axis]) * 0.5;
        let mut lo = center - span * 0.5;
        let mut hi = lo + span;

        if lo < 0.0 {
            lo = 0.0;
            hi = span;
        }
        if hi > 1.0 {
            hi = 1.0;
            lo = (1.0 - span).max(0.0);
        }

        min[axis] = lo.clamp(0.0, 1.0 - MIN_LUT_ZOOM_SPAN);
        max[axis] = hi.clamp(MIN_LUT_ZOOM_SPAN, 1.0);
    }
    (min, max)
}

/// Applies a cursor-centered zoom factor (`zoom_factor > 1.0` zooms in, `< 1.0` zooms out)
/// and immediately clamps the resulting viewport strictly within `[0.0, 1.0] × [0.0, 1.0]`.
pub fn zoom_unit_square_bounds(
    min: [f64; 2],
    max: [f64; 2],
    zoom_factor: f64,
    pivot: [f64; 2],
) -> ([f64; 2], [f64; 2]) {
    if !zoom_factor.is_finite() || zoom_factor <= 0.0 {
        return clamp_unit_square_bounds(min, max);
    }

    let (cur_min, cur_max) = clamp_unit_square_bounds(min, max);
    let mut next_min = [0.0; 2];
    let mut next_max = [1.0; 2];

    for axis in 0..2 {
        let old_span = (cur_max[axis] - cur_min[axis]).clamp(MIN_LUT_ZOOM_SPAN, MAX_LUT_ZOOM_SPAN);
        let new_span = (old_span / zoom_factor).clamp(MIN_LUT_ZOOM_SPAN, MAX_LUT_ZOOM_SPAN);
        let p = pivot[axis].clamp(0.0, 1.0);
        let rel = ((p - cur_min[axis]) / old_span).clamp(0.0, 1.0);
        next_min[axis] = p - rel * new_span;
        next_max[axis] = next_min[axis] + new_span;
    }

    clamp_unit_square_bounds(next_min, next_max)
}

/// Computes `(physical_travel_pct, ingame_output_pct)` in `0.0..=100.0` for a vertical pedal card.
///
/// Evaluates DirectInput8's exact `[lMin, lCenter, lMax]` piecewise-linear transfer curve directly
/// from `snap.raw_value` + `(saturation_pct, deadzone_pct, inverted)` whenever the user modifies
/// the saturation/deadzone sliders or `dinput8.dll`'s cooked handle sits on its post-`SetProperty`
/// `32767` (`50.0%`) state-buffer reset, ensuring the wide Output column never freezes at `50%`.
pub fn compute_live_pedal_display_percentages(
    snap: &AxisTelemetrySnapshot,
    saturation_pct: f64,
    deadzone_pct: f64,
    inverted: bool,
) -> (f64, f64) {
    let (l_min, l_max) = snap.logical_range;
    let effective_cooked_pct = if let Ok(preview_cal) = compute_smart_pedal_calibration(
        l_min,
        l_max,
        saturation_pct,
        deadzone_pct,
        inverted,
    ) {
        let model_pct = evaluate_calibrated_output_pct(snap.raw_value, preview_cal, 0, 10_000);
        if preview_cal != snap.calibration || (snap.cooked_percent - model_pct).abs() > 2.0 {
            model_pct
        } else {
            snap.cooked_percent
        }
    } else {
        snap.cooked_percent
    };

    compute_pedal_press_percentages(
        snap.raw_value,
        snap.logical_range,
        effective_cooked_pct,
        inverted,
    )
}

/// Computes the live steering wheel angle in degrees and signed travel percentage
/// (`-100..=100`, where `0` is center) for an [`AxisTelemetrySnapshot`].
///
/// Mirrors [`compute_live_pedal_display_percentages`]: if `dinput8.dll`'s cooked
/// handle sits on its `32767` (`50.0%`) buffer reset or diverges from the calibrated
/// model while the raw sensor moves away from center, evaluates the live angle
/// directly from `snap.raw_value`.
pub fn compute_live_steering_angle_and_pct(
    snap: Option<&AxisTelemetrySnapshot>,
    wheel_rotation_deg: f64,
) -> (f64, f64) {
    let Some(snap) = snap else {
        return compute_steering_angle_deg(50.0, wheel_rotation_deg);
    };

    let model_pct = evaluate_calibrated_output_pct(
        snap.raw_value,
        snap.calibration,
        snap.deadzone,
        snap.saturation,
    );

    let (l_min, l_max) = snap.logical_range;
    let l_span = ((l_max as f64) - (l_min as f64)).abs().max(1.0);
    let l_mid = ((l_min as f64) + (l_max as f64)) * 0.5;
    let raw_away_from_center = ((snap.raw_value as f64) - l_mid).abs() > (l_span * 0.001).max(1.5);
    let cooked_near_reset_mid = (snap.cooked_percent - 50.0).abs() <= 0.6;

    let effective_cooked_pct = if (cooked_near_reset_mid && raw_away_from_center)
        || ((snap.cooked_percent - model_pct).abs() > 1.5 && raw_away_from_center)
    {
        model_pct
    } else {
        snap.cooked_percent
    };

    compute_steering_angle_deg(effective_cooked_pct, wheel_rotation_deg)
}

/// Top-level `eframe::App` state for **Speed Precise**.
pub struct SpeedPreciseApp {
    pub active_screen: ActiveScreen,
    worker: HardwareWorker,

    // Connected USB controllers & live 60 Hz telemetry
    pub devices: Vec<DeviceInfo>,
    pub selected_device_idx: usize,
    pub live_telemetry: HashMap<u32, AxisTelemetrySnapshot>,
    pub cockpit_states: HashMap<usize, DeviceCockpitState>,
    initial_ffb_mode_synced: bool,

    // Unified Auto-LUT & Wheel Rotation parameters
    pub step_count: u32,
    pub step_duration_ms: u64,
    pub step_max_angle_deg: f64,
    pub step_max_force: i32,
    pub step_dry_run: bool,
    pub ffb_running: bool,
    pub step_current: u32,
    pub step_total: u32,
    pub step_state_badge: String,
    pub step_samples: Vec<StepLogSample>,
    pub loaded_csv_label: Option<String>,

    // LUT mathematical engine state & strictly bounded [0.0, 1.0] plot viewport
    pub lut_smoothing_window: usize,
    pub lut_output_steps: usize,
    pub lut_result: Option<LutResult>,
    pub lut_calc_error: Option<String>,
    pub lut_plot_min: [f64; 2],
    pub lut_plot_max: [f64; 2],
    pub ac_lut_filename: String,
    pub ac_cfg_dir: PathBuf,

    // Clean consumer status & error banners
    pub status_banner: String,
    pub error_banner: Option<String>,
}

impl SpeedPreciseApp {
    /// Initializes `SpeedPreciseApp`, applies the dark cockpit theme, and spawns the background worker.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_cockpit_dark_theme(&cc.egui_ctx);
        let worker = HardwareWorker::spawn(Some(cc.egui_ctx.clone()));
        Self::with_worker(worker)
    }

    /// Headless constructor used for deterministic unit testing of UI state transitions,
    /// Auto-LUT pipeline, and `[0.0, 1.0]` plot clamping without opening an OS window.
    pub fn new_headless_for_test(worker_cfg: WorkerConfig) -> Self {
        let worker = HardwareWorker::spawn_with_config(None, worker_cfg);
        Self::with_worker(worker)
    }

    fn with_worker(worker: HardwareWorker) -> Self {
        Self {
            active_screen: ActiveScreen::CockpitDashboard,
            worker,
            devices: Vec::new(),
            selected_device_idx: 0,
            live_telemetry: HashMap::new(),
            cockpit_states: HashMap::new(),
            initial_ffb_mode_synced: false,

            step_count: DEFAULT_STEP_LOG2_STEPS,
            step_duration_ms: DEFAULT_STEP_LOG2_DURATION_MS,
            step_max_angle_deg: DEFAULT_STEP_LOG2_MAX_ANGLE_DEG,
            step_max_force: DEFAULT_STEP_LOG2_MAX_FORCE,
            step_dry_run: false,
            ffb_running: false,
            step_current: 0,
            step_total: DEFAULT_STEP_LOG2_STEPS,
            step_state_badge: "Pronto".to_string(),
            step_samples: Vec::new(),
            loaded_csv_label: None,

            lut_smoothing_window: DEFAULT_SMOOTHING_WINDOW,
            lut_output_steps: DEFAULT_LUT_STEPS,
            lut_result: None,
            lut_calc_error: None,
            lut_plot_min: [0.0, 0.0],
            lut_plot_max: [1.0, 1.0],
            ac_lut_filename: "speed_precise.lut".to_string(),
            ac_cfg_dir: default_assetto_corsa_cfg_dir(),

            status_banner: "Procurando volantes e pedais USB...".to_string(),
            error_banner: None,
        }
    }

    /// Returns `true` ONLY while an FFB test is actively running and the contextual
    /// `[Parar Teste Agora (ESPAÇO / ESC)]` button must be displayed.
    #[inline]
    pub fn is_emergency_stop_visible(&self) -> bool {
        self.ffb_running
    }

    /// Resets the LUT chart zoom strictly back to the full `[0.0, 1.0] × [0.0, 1.0]` square.
    pub fn reset_lut_plot_zoom(&mut self) {
        self.lut_plot_min = [0.0, 0.0];
        self.lut_plot_max = [1.0, 1.0];
    }

    /// Triggers immediate sub-millisecond Force Feedback cutoff (`Space`, `ESC`, or contextual stop button).
    pub fn trigger_emergency_stop(&mut self) {
        self.worker.trigger_emergency_stop();
        if self.ffb_running {
            self.ffb_running = false;
            self.step_state_badge = "Teste Interrompido".to_string();
        }
        self.status_banner =
            "Teste interrompido — força do volante desligada imediatamente.".to_string();
    }

    /// Drains all pending [`WorkerEvent`] messages queued by the background hardware thread.
    pub fn pump_worker_events(&mut self) {
        while let Some(event) = self.worker.try_recv() {
            match event {
                WorkerEvent::DeviceListUpdated(devices) => {
                    self.handle_device_list_updated(devices);
                }
                WorkerEvent::LiveAxisTelemetry(snapshots) => {
                    for snap in snapshots {
                        self.live_telemetry.insert(snap.dw_type, snap);
                    }
                }
                WorkerEvent::CalibrationApplied(outcome) => {
                    self.error_banner = None;
                    if let Some(dev) = self.devices.get(self.selected_device_idx) {
                        if let Some(axis) = dev
                            .axes
                            .iter()
                            .find(|a| a.dw_type == outcome.axis_dw_type)
                        {
                            let (l_min, l_max) = axis.logical_range;
                            if let Some(cockpit) =
                                self.cockpit_states.get_mut(&self.selected_device_idx)
                            {
                                let entry = cockpit
                                    .pedal_states
                                    .entry(outcome.axis_dw_type)
                                    .or_insert_with(|| {
                                        PedalSliderState::from_device_axis(dev, axis)
                                    });
                                let (sat, dz) = infer_pedal_sliders_from_calibration(
                                    l_min,
                                    l_max,
                                    outcome.readback_calibration,
                                    entry.inverted,
                                );
                                entry.saturation_pct = sat;
                                entry.deadzone_pct = dz;
                            }
                        }
                    }
                }
                WorkerEvent::StepLog2StateChanged {
                    current_step,
                    total_steps,
                    state_label,
                } => {
                    if self.ffb_running {
                        self.step_current = self.step_current.max(current_step.saturating_sub(1));
                        self.step_total = total_steps;
                        self.step_state_badge = state_label;
                    }
                }
                WorkerEvent::StepLog2Progress {
                    current_step,
                    total_steps,
                    sample,
                    state_label,
                } => {
                    self.step_current = current_step;
                    self.step_total = total_steps;
                    self.step_state_badge = state_label;
                    if self.step_samples.is_empty() {
                        self.step_samples.push(StepLogSample::ZERO);
                    }
                    self.step_samples.push(sample);
                }
                WorkerEvent::StepLog2Finished(samples) => {
                    self.ffb_running = false;
                    self.step_total = samples.len().saturating_sub(1) as u32;
                    self.step_current = self.step_total;
                    self.step_state_badge = "Concluído".to_string();
                    self.error_banner = None;
                    self.reset_lut_plot_zoom();
                    self.load_samples_and_recompute_lut(
                        samples,
                        Some("Leitura automática concluída".to_string()),
                    );
                }
                WorkerEvent::StepLog2Aborted => {
                    self.ffb_running = false;
                    if !self.step_state_badge.contains("Interrompido") {
                        self.step_state_badge = "Interrompido".to_string();
                    }
                }
                WorkerEvent::StatusMessage(msg) => {
                    self.status_banner = msg;
                }
                WorkerEvent::Error(err) => {
                    self.error_banner = Some(err);
                }
            }
        }
    }

    fn handle_device_list_updated(&mut self, devices: Vec<DeviceInfo>) {
        if devices.is_empty() {
            self.selected_device_idx = 0;
            self.live_telemetry.clear();
            if !self.initial_ffb_mode_synced {
                self.step_dry_run = true;
                self.initial_ffb_mode_synced = true;
            }
        } else {
            if self.selected_device_idx >= devices.len() {
                self.selected_device_idx = devices
                    .iter()
                    .position(|d| d.has_ffb)
                    .unwrap_or(0);
            } else if !self.initial_ffb_mode_synced {
                if let Some(ffb_idx) = devices.iter().position(|d| d.has_ffb) {
                    self.selected_device_idx = ffb_idx;
                }
            }

            if !self.initial_ffb_mode_synced {
                let has_ffb = devices
                    .get(self.selected_device_idx)
                    .map(|d| d.has_ffb)
                    .unwrap_or(false);
                self.step_dry_run = !has_ffb;
                self.initial_ffb_mode_synced = true;
            }

            for (dev_idx, dev) in devices.iter().enumerate() {
                let state = self
                    .cockpit_states
                    .entry(dev_idx)
                    .or_insert_with(|| DeviceCockpitState::from_device(dev));
                for axis in &dev.axes {
                    state
                        .pedal_states
                        .entry(axis.dw_type)
                        .or_insert_with(|| PedalSliderState::from_device_axis(dev, axis));
                }
            }
        }

        self.devices = devices;
    }

    /// Loads `StepLogSample` data (from live Auto-LUT test or imported CSV) and computes the LUT in `< 1 ms`.
    pub fn load_samples_and_recompute_lut(
        &mut self,
        samples: Vec<StepLogSample>,
        source_label: Option<String>,
    ) {
        self.step_samples = samples;
        if let Some(label) = source_label {
            self.loaded_csv_label = Some(label);
        }
        self.recalculate_lut();
    }

    /// Recomputes `self.lut_result` in `< 1 ms`.
    pub fn recalculate_lut(&mut self) {
        if self.step_samples.len() < 2 {
            self.lut_result = None;
            self.lut_calc_error = None;
            return;
        }

        let cfg = LutConfig {
            output_steps: self.lut_output_steps.clamp(10, 1_000),
            smoothing_window: self.lut_smoothing_window.clamp(1, 15),
            deadzone_threshold_ratio: DEFAULT_DEADZONE_THRESHOLD_RATIO,
            center_preload_cap: None,
        };

        match generate_inverse_lut(&self.step_samples, &cfg) {
            Ok(res) => {
                self.lut_result = Some(res);
                self.lut_calc_error = None;
            }
            Err(err) => {
                self.lut_result = None;
                self.lut_calc_error = Some(format!("Não foi possível calcular a curva LUT: {err}"));
            }
        }
    }

    /// Imports an existing WheelCheck `log2.csv` file directly into the unified Auto-LUT screen.
    pub fn import_csv_from_path(&mut self, path: &Path) {
        match read_wheelcheck_csv_file(path) {
            Ok(samples) => {
                let count = samples.len().saturating_sub(1) as u32;
                self.step_current = count;
                self.step_total = count.max(1);
                self.step_state_badge = "CSV Importado".to_string();
                self.error_banner = None;
                let file_label = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("log2.csv")
                    .to_string();
                self.status_banner = format!(
                    "Arquivo `{file_label}` importado ({count} passos). Curva LUT pronta!"
                );
                self.reset_lut_plot_zoom();
                self.load_samples_and_recompute_lut(samples, Some(file_label));
            }
            Err(err) => {
                self.error_banner = Some(format!(
                    "Não foi possível ler o arquivo CSV selecionado: {err}"
                ));
            }
        }
    }

    /// Starts the unified 1-click `Step Log 2` + automatic LUT generation pipeline.
    pub fn launch_step_log2(&mut self) {
        if self.ffb_running {
            return;
        }

        if !self.step_dry_run {
            let Some(dev) = self.devices.get(self.selected_device_idx) else {
                self.error_banner = Some(
                    "Nenhum volante USB detectado. Conecte seu volante e clique em Atualizar USB."
                        .to_string(),
                );
                return;
            };
            if !dev.has_ffb {
                self.error_banner = Some(format!(
                    "O controlador `{}` não possui motor de Force Feedback. Selecione seu volante principal no topo da janela.",
                    dev.instance_name
                ));
                return;
            }
        }

        self.error_banner = None;
        self.active_screen = ActiveScreen::AutoLutStudio;
        self.ffb_running = true;
        self.step_current = 0;
        self.step_total = self.step_count;
        self.step_state_badge = "Preparando volante".to_string();
        self.step_samples = vec![StepLogSample::ZERO];
        self.loaded_csv_label = None;

        self.worker.start_step_log2(
            self.selected_device_idx,
            self.step_count,
            self.step_duration_ms,
            self.step_max_angle_deg,
            self.step_max_force,
            self.step_dry_run,
        );
    }

    // ========================================================================
    // Top Header & Bottom Status Bar
    // ========================================================================

    fn render_top_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("speed_precise_clean_header")
            .frame(
                egui::Frame::none()
                    .fill(COLOR_BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(16.0_f32, 10.0_f32))
                    .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 1. Clean App Brand
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("SPEED PRECISE")
                                .size(18.5_f32)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new("Calibração de Volante, Pedais & Force Feedback")
                                .size(11.0_f32)
                                .color(COLOR_TEXT_MUTED),
                        );
                    });

                    ui.add_space(16.0_f32);
                    ui.separator();
                    ui.add_space(10.0_f32);

                    // 2. Clean 2-Screen Navigation Switcher
                    ui.add_enabled_ui(!self.ffb_running, |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0_f32;
                        self.render_screen_nav_button(
                            ui,
                            ActiveScreen::CockpitDashboard,
                            "Volante e Pedais",
                        );
                        let lut_nav_label = if self.lut_result.is_some() {
                            "Gerador de LUT (Force Feedback)  •  Pronta"
                        } else {
                            "Gerador de LUT (Force Feedback)"
                        };
                        self.render_screen_nav_button(ui, ActiveScreen::AutoLutStudio, lut_nav_label);
                    });

                    // 3. Right-Aligned Clean Controller Selector & [Atualizar USB] (NO permanent Red Stop button!)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut newly_selected: Option<usize> = None;

                        ui.add_enabled_ui(!self.ffb_running, |ui| {
                            if add_icon_button(
                                ui,
                                VectorIcon::Refresh,
                                "Atualizar USB",
                                12.5_f32,
                                Vec2::new(124.0_f32, 30.0_f32),
                                None,
                                None,
                                5.0_f32,
                            )
                            .on_hover_text("Procurar volantes e pedais USB conectados")
                            .clicked()
                            {
                                self.error_banner = None;
                                self.worker.send(GuiCommand::RefreshDevices);
                            }

                            let selected_label =
                                if let Some(dev) = self.devices.get(self.selected_device_idx) {
                                    format_clean_device_label(dev)
                                } else {
                                    "Nenhum controlador USB".to_string()
                                };

                            egui::ComboBox::from_id_salt("clean_usb_controller_combo")
                                .selected_text(RichText::new(selected_label).size(12.5_f32))
                                .width(270.0_f32)
                                .show_ui(ui, |ui| {
                                    if self.devices.is_empty() {
                                        ui.label("Nenhum dispositivo conectado");
                                    }
                                    for (idx, dev) in self.devices.iter().enumerate() {
                                        let label = format_clean_device_label(dev);
                                        if ui
                                            .selectable_label(
                                                idx == self.selected_device_idx,
                                                label,
                                            )
                                            .clicked()
                                        {
                                            newly_selected = Some(idx);
                                        }
                                    }
                                });
                        });

                        if let Some(new_idx) = newly_selected {
                            if new_idx != self.selected_device_idx {
                                self.selected_device_idx = new_idx;
                                self.live_telemetry.clear();
                                if let Some(dev) = self.devices.get(new_idx) {
                                    self.step_dry_run = !dev.has_ffb;
                                }
                                self.worker.send(GuiCommand::SelectDevice(new_idx));
                            }
                        }
                    });
                });
            });
    }

    fn render_screen_nav_button(
        &mut self,
        ui: &mut egui::Ui,
        screen: ActiveScreen,
        label: &str,
    ) {
        let is_active = self.active_screen == screen;
        let fill = if is_active {
            COLOR_ACCENT_SURFACE
        } else {
            COLOR_BG_CARD
        };
        let text_color = if is_active {
            Color32::WHITE
        } else {
            COLOR_TEXT_SECONDARY
        };
        let border = if is_active {
            COLOR_ACCENT_BORDER
        } else {
            COLOR_BORDER_SUBTLE
        };

        let btn = egui::Button::new(RichText::new(label).size(13.0_f32).strong().color(text_color))
            .fill(fill)
            .rounding(Rounding::same(6.0_f32))
            .stroke(Stroke::new(1.0_f32, border))
            .min_size(Vec2::new(165.0_f32, 32.0_f32));

        if ui.add(btn).clicked() {
            self.active_screen = screen;
        }
    }

    fn render_bottom_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("speed_precise_clean_status")
            .frame(
                egui::Frame::none()
                    .fill(COLOR_BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(16.0_f32, 6.0_f32))
                    .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                if let Some(err) = self.error_banner.clone() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let err_color = Color32::from_rgb(255, 118, 117);
                        paint_warning_icon(ui, err_color);
                        ui.label(RichText::new(err).color(err_color).strong());
                        if ui.small_button("Fechar").clicked() {
                            self.error_banner = None;
                        }
                    });
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let dot_color = if self.ffb_running {
                        COLOR_BADGE_WARN
                    } else {
                        COLOR_BADGE_OK
                    };
                    paint_status_led(ui, dot_color);
                    ui.label(
                        RichText::new(&self.status_banner)
                            .size(12.0_f32)
                            .color(COLOR_TEXT_SECONDARY),
                    );
                });
            });
    }

    // ========================================================================
    // Screen 1 — Volante (Horizontal) + 3 Pedais (Verticais: Embreagem, Freio, Acelerador)
    // ========================================================================

    fn render_screen_cockpit(&mut self, ui: &mut egui::Ui) {
        let Some(dev) = self.devices.get(self.selected_device_idx).cloned() else {
            egui::Frame::none()
                .fill(COLOR_BG_CARD)
                .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE))
                .rounding(Rounding::same(8.0_f32))
                .inner_margin(24.0_f32)
                .show(ui, |ui| {
                    ui.heading("Nenhum Volante ou Pedal Conectado");
                    ui.add_space(8.0_f32);
                    ui.label(
                        "Conecte seu volante ou pedaleira USB ao computador e clique em \"Atualizar USB\" no canto superior direito.",
                    );
                    ui.add_space(6.0_f32);
                    ui.label(
                        RichText::new(
                            "Você também pode abrir a tela \"Gerador de LUT (Force Feedback)\" para importar um arquivo CSV ou testar o gerador.",
                        )
                        .color(COLOR_TEXT_MUTED),
                    );
                });
            return;
        };

        if dev.axes.is_empty() {
            ui.label("O controlador selecionado não possui eixos analógicos.");
            return;
        }

        let dev_idx = self.selected_device_idx;
        self.cockpit_states
            .entry(dev_idx)
            .or_insert_with(|| DeviceCockpitState::from_device(&dev));

        let mut pending_commands: Vec<GuiCommand> = Vec::new();

        // --------------------------------------------------------------------
        // Top Section: VOLANTE (Horizontal Center-Zero Gauge)
        // --------------------------------------------------------------------
        self.render_horizontal_steering_card(ui, &dev, dev_idx, &mut pending_commands);

        ui.add_space(10.0_f32);

        // --------------------------------------------------------------------
        // Bottom Section Header: PEDAIS + Global Quick Actions
        // --------------------------------------------------------------------
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("PEDAIS")
                    .size(14.5_f32)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new("•  Barra fina: curso físico do sensor  |  Barra principal: resposta enviada ao jogo (0% a 100%)")
                    .size(12.0_f32)
                    .color(COLOR_TEXT_MUTED),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply_clicked = ui
                    .add_enabled_ui(!self.ffb_running, |ui| {
                        add_icon_button(
                            ui,
                            VectorIcon::Check,
                            "Aplicar nos Pedais",
                            12.5_f32,
                            Vec2::new(158.0_f32, 28.0_f32),
                            Some(COLOR_ACCENT_SURFACE),
                            Some(Stroke::new(1.0_f32, COLOR_ACCENT_BORDER)),
                            5.0_f32,
                        )
                        .on_hover_text(
                            "Grava imediatamente a Saturação e Zona Morta dos 3 pedais no Windows",
                        )
                        .clicked()
                    })
                    .inner;

                if apply_clicked {
                    if let Some(cockpit) = self.cockpit_states.get(&dev_idx) {
                        for role in PedalRole::ALL_LEFT_TO_RIGHT {
                            if let Some(axis_id) = cockpit.axis_for_pedal(role) {
                                if let (Some(axis), Some(pstate)) = (
                                    dev.axes.iter().find(|a| a.dw_type == axis_id),
                                    cockpit.pedal_states.get(&axis_id),
                                ) {
                                    let (l_min, l_max) = axis.logical_range;
                                    if let Ok(cal) = compute_smart_pedal_calibration(
                                        l_min,
                                        l_max,
                                        pstate.saturation_pct,
                                        pstate.deadzone_pct,
                                        pstate.inverted,
                                    ) {
                                        pending_commands.push(GuiCommand::ApplyAxisCalibration {
                                            device_idx: dev_idx,
                                            axis_id,
                                            min: cal.min,
                                            center: cal.center,
                                            max: cal.max,
                                            deadzone: Some(0),
                                            saturation: Some(10_000),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if ui
                    .add_enabled(
                        !self.ffb_running,
                        egui::Button::new("Restaurar Padrão dos Pedais")
                            .rounding(Rounding::same(5.0_f32)),
                    )
                    .on_hover_text("Volta Saturação para 100% e Zona Morta para 0% nos 3 pedais")
                    .clicked()
                {
                    if let Some(cockpit) = self.cockpit_states.get_mut(&dev_idx) {
                        for role in PedalRole::ALL_LEFT_TO_RIGHT {
                            if let Some(axis_id) = cockpit.axis_for_pedal(role) {
                                if let Some(pstate) = cockpit.pedal_states.get_mut(&axis_id) {
                                    pstate.saturation_pct = 100.0;
                                    pstate.deadzone_pct = 0.0;
                                }
                                pending_commands.push(GuiCommand::RestoreDefaultCalibration {
                                    device_idx: dev_idx,
                                    axis_id,
                                });
                            }
                        }
                    }
                }
            });
        });

        ui.add_space(6.0_f32);

        // --------------------------------------------------------------------
        // Bottom 3 Vertical Pedal Columns (Strictly Left-to-Right):
        // 1. Embreagem (Esquerda) | 2. Freio (Meio) | 3. Acelerador (Direita)
        // --------------------------------------------------------------------
        let column_avail_h = ui.available_height().max(290.0_f32);
        ui.columns(3, |cols| {
            for (col_idx, role) in PedalRole::ALL_LEFT_TO_RIGHT.into_iter().enumerate() {
                self.render_vertical_pedal_card(
                    &mut cols[col_idx],
                    &dev,
                    dev_idx,
                    role,
                    column_avail_h,
                    &mut pending_commands,
                );
            }
        });

        for cmd in pending_commands {
            self.error_banner = None;
            self.worker.send(cmd);
        }
    }

    fn render_horizontal_steering_card(
        &mut self,
        ui: &mut egui::Ui,
        dev: &DeviceInfo,
        dev_idx: usize,
        pending_commands: &mut Vec<GuiCommand>,
    ) {
        let steering_axis_id = self
            .cockpit_states
            .get(&dev_idx)
            .and_then(|c| c.mapping.steering);

        let steering_axis = steering_axis_id
            .and_then(|id| dev.axes.iter().find(|a| a.dw_type == id))
            .cloned();

        let steering_snap = steering_axis
            .as_ref()
            .and_then(|ax| self.live_telemetry.get(&ax.dw_type));

        let (angle_deg, signed_pct) =
            compute_live_steering_angle_and_pct(steering_snap, self.step_max_angle_deg);

        egui::Frame::none()
            .fill(COLOR_BG_CARD)
            .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE))
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(16.0_f32, 12.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("VOLANTE (DIREÇÃO)")
                            .size(14.5_f32)
                            .strong()
                            .color(Color32::WHITE),
                    );

                    ui.add_space(8.0_f32);

                    // Discreet Steering Axis Selector
                    let current_axis_name = steering_axis
                        .as_ref()
                        .map(|a| clean_axis_display_name(&a.name, a.standard_label))
                        .unwrap_or_else(|| "Não atribuído".to_string());

                    egui::ComboBox::from_id_salt("cockpit_steering_axis_combo")
                        .selected_text(
                            RichText::new(format!("Eixo: {current_axis_name}"))
                                .size(11.5_f32)
                                .color(COLOR_TEXT_MUTED),
                        )
                        .width(175.0_f32)
                        .show_ui(ui, |ui| {
                            if let Some(cockpit) = self.cockpit_states.get_mut(&dev_idx) {
                                if ui
                                    .selectable_label(
                                        cockpit.mapping.steering.is_none(),
                                        "Nenhum",
                                    )
                                    .clicked()
                                {
                                    cockpit.mapping.steering = None;
                                }
                                for ax in &dev.axes {
                                    let label = clean_axis_display_name(&ax.name, ax.standard_label);
                                    if ui
                                        .selectable_label(
                                            cockpit.mapping.steering == Some(ax.dw_type),
                                            label,
                                        )
                                        .clicked()
                                    {
                                        cockpit.mapping.steering = Some(ax.dw_type);
                                    }
                                }
                            }
                        });

                    ui.add_space(8.0_f32);

                    // Quick Wheel Rotation Pills (`540°`, `900°`, `1080°`)
                    ui.label(
                        RichText::new("Giro:")
                            .size(11.5_f32)
                            .color(COLOR_TEXT_MUTED),
                    );
                    for preset in [540.0_f64, 900.0_f64, 1080.0_f64] {
                        let active = (self.step_max_angle_deg - preset).abs() < 0.5;
                        if ui
                            .selectable_label(active, format!("{preset:.0}°"))
                            .clicked()
                        {
                            self.step_max_angle_deg = preset;
                        }
                    }

                    if let Some(ref ax) = steering_axis {
                        if ui
                            .add_enabled(
                                !self.ffb_running,
                                egui::Button::new(RichText::new("Restaurar Centro").size(11.5_f32)),
                            )
                            .on_hover_text("Restaura a calibração central original do volante")
                            .clicked()
                        {
                            pending_commands.push(GuiCommand::RestoreDefaultCalibration {
                                device_idx: dev_idx,
                                axis_id: ax.dw_type,
                            });
                        }
                    }

                    // Live Angle & Percentage Readout on the Right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("({signed_pct:+.0}%)"))
                                .size(13.5_f32)
                                .monospace()
                                .color(COLOR_TEXT_MUTED),
                        );
                        ui.add_space(4.0_f32);
                        ui.label(
                            RichText::new(format!("{angle_deg:+6.1}°"))
                                .size(22.0_f32)
                                .monospace()
                                .strong()
                                .color(COLOR_STEERING_FILL),
                        );
                    });
                });

                ui.add_space(8.0_f32);

                // Custom Horizontal Center-Zero Steering Bar
                paint_horizontal_steering_gauge(ui, signed_pct, self.step_max_angle_deg);
            });
    }

    fn render_vertical_pedal_card(
        &mut self,
        ui: &mut egui::Ui,
        dev: &DeviceInfo,
        dev_idx: usize,
        role: PedalRole,
        column_avail_h: f32,
        pending_commands: &mut Vec<GuiCommand>,
    ) {
        let accent = role.accent_color();
        let assigned_axis_id = self
            .cockpit_states
            .get(&dev_idx)
            .and_then(|c| c.axis_for_pedal(role));

        let assigned_axis = assigned_axis_id
            .and_then(|id| dev.axes.iter().find(|a| a.dw_type == id))
            .cloned();

        let (saturation_pct, deadzone_pct, inverted) = assigned_axis
            .as_ref()
            .and_then(|ax| {
                self.cockpit_states
                    .get(&dev_idx)
                    .and_then(|c| c.pedal_states.get(&ax.dw_type))
            })
            .map(|s| (s.saturation_pct, s.deadzone_pct, s.inverted))
            .unwrap_or((100.0, 0.0, dev.vid == 0x046D));

        let (physical_pct, ingame_pct) = if let Some(ref ax) = assigned_axis {
            if let Some(snap) = self.live_telemetry.get(&ax.dw_type) {
                compute_live_pedal_display_percentages(
                    snap,
                    saturation_pct,
                    deadzone_pct,
                    inverted,
                )
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        egui::Frame::none()
            .fill(COLOR_BG_CARD)
            .stroke(Stroke::new(1.2_f32, accent.gamma_multiply(0.45_f32)))
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0_f32, 12.0_f32))
            .show(ui, |ui| {
                // 1. Pedal Header & Live 0%..100% Output Readout
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(role.title())
                                .size(15.0_f32)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(role.position_hint())
                                .size(11.0_f32)
                                .color(COLOR_TEXT_MUTED),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!("{ingame_pct:3.0}%"))
                                    .size(25.0_f32)
                                    .monospace()
                                    .strong()
                                    .color(accent),
                            );
                            ui.label(
                                RichText::new(format!("Físico: {physical_pct:3.0}%"))
                                    .size(11.0_f32)
                                    .color(COLOR_TEXT_MUTED),
                            );
                        });
                    });
                });

                ui.add_space(8.0_f32);

                // 2. Vertical Dual-Bar Telemetry Gauge (0% Bottom -> 100% Top)
                let bar_height = (column_avail_h - 195.0_f32).clamp(135.0_f32, 330.0_f32);
                paint_vertical_pedal_bars(
                    ui,
                    bar_height,
                    physical_pct,
                    ingame_pct,
                    saturation_pct,
                    deadzone_pct,
                    accent,
                );

                ui.add_space(10.0_f32);

                // 3. Clean Saturation & Deadzone Sliders
                if let Some(ref ax) = assigned_axis {
                    let mut next_sat = saturation_pct;
                    let mut next_dz = deadzone_pct;
                    let mut next_inv = inverted;
                    let mut apply_now = false;

                    ui.add_enabled_ui(!self.ffb_running, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Curso Máximo (Saturação):")
                                    .size(12.0_f32)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!("{next_sat:.0}%"))
                                            .size(12.0_f32)
                                            .strong()
                                            .color(accent),
                                    );
                                },
                            );
                        });
                        let sat_resp = ui
                            .add(
                                egui::Slider::new(&mut next_sat, 10.0..=100.0)
                                    .show_value(false)
                                    .step_by(1.0),
                            )
                            .on_hover_text(
                                "Define quanto você precisa pisar no pedal físico para alcançar 100% de força dentro do jogo (Ex.: 40% ideal para freio duro).",
                            );

                        ui.add_space(4.0_f32);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Zona Morta Inicial:")
                                    .size(12.0_f32)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!("{next_dz:.0}%"))
                                            .size(12.0_f32)
                                            .color(COLOR_TEXT_MUTED),
                                    );
                                },
                            );
                        });
                        let dz_resp = ui
                            .add(
                                egui::Slider::new(&mut next_dz, 0.0..=30.0)
                                    .show_value(false)
                                    .step_by(1.0),
                            )
                            .on_hover_text(
                                "Ignora pequenas folgas iniciais ao apoiar o pé no pedal.",
                            );

                        if next_sat < next_dz + 5.0 {
                            next_sat = (next_dz + 5.0).min(100.0);
                        }

                        if sat_resp.drag_stopped()
                            || dz_resp.drag_stopped()
                            || (sat_resp.changed() && !sat_resp.dragged())
                            || (dz_resp.changed() && !dz_resp.dragged())
                        {
                            apply_now = true;
                        }

                        // Discreet Collapsible Options (Axis Swap & Pedal Polarity)
                        ui.add_space(4.0_f32);
                        egui::CollapsingHeader::new(
                            RichText::new("Opções do pedal")
                                .size(11.5_f32)
                                .color(COLOR_TEXT_MUTED),
                        )
                        .id_salt(("pedal_opts", role))
                        .default_open(false)
                        .show(ui, |ui| {
                            let current_name =
                                clean_axis_display_name(&ax.name, ax.standard_label);
                            egui::ComboBox::from_id_salt(("pedal_axis_swap", role))
                                .selected_text(format!("Eixo: {current_name}"))
                                .width(160.0_f32)
                                .show_ui(ui, |ui| {
                                    if let Some(cockpit) = self.cockpit_states.get_mut(&dev_idx) {
                                        if ui.selectable_label(false, "Nenhum").clicked() {
                                            cockpit.set_axis_for_pedal(role, None);
                                        }
                                        for candidate in &dev.axes {
                                            let lbl = clean_axis_display_name(
                                                &candidate.name,
                                                candidate.standard_label,
                                            );
                                            if ui
                                                .selectable_label(
                                                    candidate.dw_type == ax.dw_type,
                                                    lbl,
                                                )
                                                .clicked()
                                            {
                                                cockpit.set_axis_for_pedal(
                                                    role,
                                                    Some(candidate.dw_type),
                                                );
                                            }
                                        }
                                    }
                                });

                            if ui
                                .checkbox(&mut next_inv, "Inverter sentido do sensor")
                                .on_hover_text(
                                    "Ativado por padrão em pedais Logitech (G29/G920), onde o sensor lê valor máximo com o pé solto.",
                                )
                                .changed()
                            {
                                apply_now = true;
                            }
                        });
                    });

                    // Persist updated slider values in UI state & send live calibration when released
                    if let Some(cockpit) = self.cockpit_states.get_mut(&dev_idx) {
                        if let Some(state) = cockpit.pedal_states.get_mut(&ax.dw_type) {
                            state.saturation_pct = next_sat;
                            state.deadzone_pct = next_dz;
                            state.inverted = next_inv;
                        }
                    }

                    if apply_now {
                        let (l_min, l_max) = ax.logical_range;
                        if let Ok(cal) = compute_smart_pedal_calibration(
                            l_min, l_max, next_sat, next_dz, next_inv,
                        ) {
                            pending_commands.push(GuiCommand::ApplyAxisCalibration {
                                device_idx: dev_idx,
                                axis_id: ax.dw_type,
                                min: cal.min,
                                center: cal.center,
                                max: cal.max,
                                deadzone: Some(0),
                                saturation: Some(10_000),
                            });
                        }
                    }
                } else {
                    ui.label(
                        RichText::new("Nenhum eixo atribuído a este pedal.")
                            .size(12.0_f32)
                            .color(COLOR_TEXT_MUTED),
                    );
                    egui::ComboBox::from_id_salt(("pedal_assign_empty", role))
                        .selected_text("Escolher eixo...")
                        .show_ui(ui, |ui| {
                            if let Some(cockpit) = self.cockpit_states.get_mut(&dev_idx) {
                                for candidate in &dev.axes {
                                    let lbl = clean_axis_display_name(
                                        &candidate.name,
                                        candidate.standard_label,
                                    );
                                    if ui.selectable_label(false, lbl).clicked() {
                                        cockpit.set_axis_for_pedal(role, Some(candidate.dw_type));
                                    }
                                }
                            }
                        });
                }
            });
    }

    // ========================================================================
    // Screen 2 — Unified Auto-LUT Generator (1-Click Test + Bounded [0, 1] Plot)
    // ========================================================================

    fn render_screen_autolut(&mut self, ui: &mut egui::Ui) {
        // --------------------------------------------------------------------
        // 1. Minimal Control Card + Contextual Progress / Stop Strip
        // --------------------------------------------------------------------
        egui::Frame::none()
            .fill(COLOR_BG_CARD)
            .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE))
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(16.0_f32, 12.0_f32))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(!self.ffb_running, |ui| {
                        // Input 1: Passos de Leitura (default 50)
                        ui.label(RichText::new("Passos de Leitura:").strong());
                        ui.add(
                            egui::Slider::new(&mut self.step_count, 10..=100)
                                .clamping(egui::SliderClamping::Always),
                        );

                        ui.add_space(10.0_f32);
                        ui.separator();
                        ui.add_space(10.0_f32);

                        // Input 2: Ângulo do Volante (°)
                        ui.label(RichText::new("Ângulo do Volante:").strong());
                        for preset in [540.0_f64, 900.0_f64, 1080.0_f64] {
                            let active = (self.step_max_angle_deg - preset).abs() < 0.5;
                            if ui
                                .selectable_label(active, format!("{preset:.0}°"))
                                .clicked()
                            {
                                self.step_max_angle_deg = preset;
                            }
                        }
                        ui.add(
                            egui::DragValue::new(&mut self.step_max_angle_deg)
                                .range(180.0..=2_520.0)
                                .speed(10.0)
                                .suffix("°")
                                .max_decimals(0),
                        );

                        ui.add_space(14.0_f32);

                        // Input 3: Primary CTA [Iniciar Teste e Gerar LUT] + Discreet [Importar CSV]
                        if add_icon_button(
                            ui,
                            VectorIcon::Play,
                            "Iniciar Teste e Gerar LUT",
                            13.5_f32,
                            Vec2::new(225.0_f32, 34.0_f32),
                            Some(COLOR_BADGE_OK),
                            None,
                            6.0_f32,
                        )
                        .on_hover_text(
                            "Executa a leitura automática do motor do volante e exibe imediatamente a curva LUT pronta.",
                        )
                        .clicked()
                        {
                            self.launch_step_log2();
                        }

                        if add_icon_button(
                            ui,
                            VectorIcon::Folder,
                            "Importar CSV",
                            12.5_f32,
                            Vec2::new(130.0_f32, 34.0_f32),
                            None,
                            None,
                            6.0_f32,
                        )
                        .on_hover_text("Carregar um arquivo CSV existente do WheelCheck")
                        .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Importar arquivo CSV de leitura do volante")
                                .add_filter("Arquivo CSV (*.csv)", &["csv"])
                                .pick_file()
                            {
                                self.import_csv_from_path(&path);
                            }
                        }
                    });
                });

                // Contextual Progress Bar & Emergency Stop Button — Shown ONLY while `ffb_running == true`!
                if self.is_emergency_stop_visible() {
                    ui.add_space(10.0_f32);
                    ui.horizontal(|ui| {
                        // Anchor Emergency Stop Button on the right first so it NEVER gets clipped
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if add_icon_button(
                                ui,
                                VectorIcon::Stop,
                                "Parar Teste Agora (ESPAÇO / ESC)",
                                13.0_f32,
                                Vec2::new(268.0_f32, 32.0_f32),
                                Some(COLOR_EMERGENCY_ACTIVE),
                                Some(Stroke::new(1.5_f32, Color32::from_rgb(255, 145, 145))),
                                6.0_f32,
                            )
                            .clicked()
                            {
                                self.trigger_emergency_stop();
                            }

                            ui.add_space(10.0_f32);

                            let denom = self.step_total.max(1) as f32;
                            let frac = ((self.step_current as f32) / denom).clamp(0.0_f32, 1.0_f32);
                            let bar = egui::ProgressBar::new(frac)
                                .fill(COLOR_ACCENT_SURFACE)
                                .desired_height(28.0_f32)
                                .desired_width(ui.available_width())
                                .text(
                                    RichText::new(format!(
                                        "Passo {} de {}  ({:.0}%)  —  {}",
                                        self.step_current,
                                        self.step_total,
                                        frac * 100.0_f32,
                                        self.step_state_badge
                                    ))
                                    .strong()
                                    .color(Color32::WHITE),
                                );
                            ui.add(bar);
                        });
                    });
                } else if self.step_dry_run && self.devices.iter().all(|d| !d.has_ffb) {
                    ui.add_space(6.0_f32);
                    ui.label(
                        RichText::new(
                            "Modo de demonstração ativo (nenhum volante com Force Feedback detectado no USB).",
                        )
                        .size(11.5_f32)
                        .color(COLOR_TEXT_MUTED),
                    );
                }
            });

        ui.add_space(8.0_f32);

        // --------------------------------------------------------------------
        // 2. Bottom Output Action Bar (Placed in a clean fixed card or reserved height)
        //    We reserve 58 px below the chart so [Aplicar no Assetto Corsa] & [Salvar .LUT]
        //    are always visible without vertical scrolling.
        // --------------------------------------------------------------------
        let chart_card_height = (ui.available_height() - 62.0_f32).max(240.0_f32);

        // --------------------------------------------------------------------
        // 3. Single Clean LUT Chart Strictly Locked Inside [0.0, 1.0] × [0.0, 1.0]
        // --------------------------------------------------------------------
        egui::Frame::none()
            .fill(COLOR_BG_CARD)
            .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE))
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0_f32, 10.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Curva Final de Correção Force Feedback (LUT)")
                            .size(14.5_f32)
                            .strong()
                            .color(Color32::WHITE),
                    );

                    if let Some(ref lut) = self.lut_result {
                        ui.add_space(10.0_f32);
                        egui::Frame::none()
                            .fill(COLOR_BADGE_WARN.gamma_multiply(0.22_f32))
                            .stroke(Stroke::new(1.0_f32, COLOR_BADGE_WARN))
                            .rounding(Rounding::same(4.0_f32))
                            .inner_margin(egui::Margin::symmetric(8.0_f32, 2.0_f32))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "Zona Morta Compensada: {:.1}%",
                                        lut.deadzone_percent
                                    ))
                                    .size(11.5_f32)
                                    .strong()
                                    .color(Color32::from_rgb(255, 214, 138)),
                                );
                            });
                    } else if let Some(ref err) = self.lut_calc_error {
                        ui.add_space(10.0_f32);
                        ui.label(
                            RichText::new(err)
                                .size(12.0_f32)
                                .color(Color32::from_rgb(255, 118, 117)),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_zoomed = (self.lut_plot_max[0] - self.lut_plot_min[0]) < 0.999
                            || (self.lut_plot_max[1] - self.lut_plot_min[1]) < 0.999;

                        let reset_clicked = ui
                            .add_enabled_ui(is_zoomed, |ui| {
                                add_icon_button(
                                    ui,
                                    VectorIcon::ResetZoom,
                                    "Resetar Zoom",
                                    12.0_f32,
                                    Vec2::new(118.0_f32, 26.0_f32),
                                    None,
                                    None,
                                    5.0_f32,
                                )
                                .clicked()
                            })
                            .inner;

                        if reset_clicked {
                            self.reset_lut_plot_zoom();
                        }

                        ui.add_space(8.0_f32);
                        ui.label(
                            RichText::new("Roda do mouse: Zoom  •  Arrastar: Mover")
                                .size(11.5_f32)
                                .color(COLOR_TEXT_MUTED),
                        );
                    });
                });

                ui.add_space(6.0_f32);

                let plot_height = (chart_card_height - 46.0_f32).max(190.0_f32);
                let plot = Plot::new("clean_bounded_lut_plot")
                    .height(plot_height)
                    .legend(Legend::default())
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .allow_boxed_zoom(false)
                    .allow_double_click_reset(false)
                    .set_margin_fraction(Vec2::ZERO)
                    .x_axis_label("Força Enviada pelo Jogo (0.00 a 1.00)")
                    .y_axis_label("Resposta do Volante / Torque Corrigido (0.00 a 1.00)");

                let mut bounds_min = self.lut_plot_min;
                let mut bounds_max = self.lut_plot_max;
                let lut_snapshot = self.lut_result.clone();

                plot.show(ui, |plot_ui| {
                    // 1. Double-click resets strictly to [0.0, 1.0] x [0.0, 1.0]
                    if plot_ui.response().double_clicked() {
                        bounds_min = [0.0, 0.0];
                        bounds_max = [1.0, 1.0];
                    }

                    // 2. Left-drag pans strictly inside [0.0, 1.0] x [0.0, 1.0]
                    if plot_ui.response().dragged_by(egui::PointerButton::Primary) {
                        let drag_delta = plot_ui.pointer_coordinate_drag_delta();
                        bounds_min[0] -= drag_delta.x as f64;
                        bounds_max[0] -= drag_delta.x as f64;
                        bounds_min[1] -= drag_delta.y as f64;
                        bounds_max[1] -= drag_delta.y as f64;
                        (bounds_min, bounds_max) =
                            clamp_unit_square_bounds(bounds_min, bounds_max);
                    }

                    // 3. Mouse-wheel OR pinch/Ctrl+wheel zooms smoothly around cursor inside [0.0, 1.0]
                    if plot_ui.response().hovered() {
                        let (zoom_delta, scroll_y) = plot_ui
                            .ctx()
                            .input(|i| (i.zoom_delta(), i.smooth_scroll_delta.y));

                        let mut z = zoom_delta as f64;
                        if (z - 1.0).abs() < 1e-4 && scroll_y.abs() > 0.0_f32 {
                            z = ((scroll_y as f64) * 0.0045).exp().clamp(0.5, 2.0);
                        }

                        if (z - 1.0).abs() > 1e-4 {
                            let pivot = plot_ui
                                .pointer_coordinate()
                                .map(|p| [p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0)])
                                .unwrap_or([
                                    (bounds_min[0] + bounds_max[0]) * 0.5,
                                    (bounds_min[1] + bounds_max[1]) * 0.5,
                                ]);
                            (bounds_min, bounds_max) =
                                zoom_unit_square_bounds(bounds_min, bounds_max, z, pivot);
                        }
                    }

                    // 4. Hard per-frame clamp before rendering so viewport NEVER leaves [0.0, 1.0]
                    (bounds_min, bounds_max) = clamp_unit_square_bounds(bounds_min, bounds_max);
                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(bounds_min, bounds_max));

                    // Curve 1: Reference 1:1 Linear Diagonal
                    plot_ui.line(
                        Line::new(PlotPoints::from(vec![[0.0, 0.0], [1.0, 1.0]]))
                            .name("Referência Linear Ideal (1:1)")
                            .color(COLOR_PLOT_IDEAL)
                            .style(LineStyle::Dashed { length: 8.0_f32 })
                            .width(1.6_f32),
                    );

                    if let Some(ref lut) = lut_snapshot {
                        // Curve 2: Raw Wheel Response
                        let raw_pts: Vec<[f64; 2]> =
                            lut.raw_response_norm.iter().map(|&(x, y)| [x, y]).collect();
                        plot_ui.line(
                            Line::new(PlotPoints::from(raw_pts))
                                .name("Resposta Bruta do Volante")
                                .color(COLOR_PLOT_RAW)
                                .width(2.1_f32),
                        );

                        // Curve 3: Final Corrected Inverse LUT
                        let inv_pts: Vec<[f64; 2]> =
                            lut.points.iter().map(|pt| [pt.x, pt.y]).collect();
                        plot_ui.line(
                            Line::new(PlotPoints::from(inv_pts))
                                .name("Curva LUT Corrigida (Final)")
                                .color(COLOR_PLOT_LUT)
                                .width(2.8_f32),
                        );
                    }
                });

                self.lut_plot_min = bounds_min;
                self.lut_plot_max = bounds_max;
            });

        ui.add_space(8.0_f32);

        // --------------------------------------------------------------------
        // 4. Clean Output Actions: [Aplicar no Assetto Corsa] & [Salvar Arquivo .LUT]
        // --------------------------------------------------------------------
        egui::Frame::none()
            .fill(COLOR_BG_CARD)
            .stroke(Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE))
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(16.0_f32, 10.0_f32))
            .show(ui, |ui| {
                let can_export = !self.ffb_running && self.lut_result.is_some();

                ui.horizontal_wrapped(|ui| {
                    let apply_ac_clicked = ui
                        .add_enabled_ui(can_export, |ui| {
                            add_icon_button(
                                ui,
                                VectorIcon::ApplyCheck,
                                "Aplicar no Assetto Corsa",
                                13.5_f32,
                                Vec2::new(235.0_f32, 34.0_f32),
                                Some(COLOR_BADGE_OK),
                                None,
                                6.0_f32,
                            )
                            .on_hover_text(
                                "Salva a LUT e ativa automaticamente o filtro FFB (ff_post_process.ini) no Assetto Corsa.",
                            )
                            .clicked()
                        })
                        .inner;

                    if apply_ac_clicked {
                        if let Some(ref lut) = self.lut_result {
                            match inject_assetto_corsa_lut(
                                lut,
                                &self.ac_lut_filename,
                                Some(&self.ac_cfg_dir),
                            ) {
                                Ok(outcome) => {
                                    self.error_banner = None;
                                    self.status_banner = format!(
                                        "LUT aplicada com sucesso no Assetto Corsa (`{}`)!",
                                        outcome.lut_path.display()
                                    );
                                }
                                Err(err) => {
                                    self.error_banner = Some(format!(
                                        "Não foi possível aplicar no Assetto Corsa: {err}"
                                    ));
                                }
                            }
                        }
                    }

                    let save_lut_clicked = ui
                        .add_enabled_ui(can_export, |ui| {
                            add_icon_button(
                                ui,
                                VectorIcon::Save,
                                "Salvar Arquivo .LUT",
                                13.5_f32,
                                Vec2::new(195.0_f32, 34.0_f32),
                                Some(COLOR_ACCENT_SURFACE),
                                Some(Stroke::new(1.0_f32, COLOR_ACCENT_BORDER)),
                                6.0_f32,
                            )
                            .on_hover_text("Salvar o arquivo .lut em qualquer pasta do computador")
                            .clicked()
                        })
                        .inner;

                    if save_lut_clicked {
                        if let Some(ref lut) = self.lut_result {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Salvar arquivo Force Feedback (.lut)")
                                .set_file_name(&self.ac_lut_filename)
                                .add_filter("Force Feedback LUT (*.lut)", &["lut"])
                                .save_file()
                            {
                                match write_lut_file(&path, lut) {
                                    Ok(()) => {
                                        self.error_banner = None;
                                        self.status_banner = format!(
                                            "Arquivo `.lut` salvo com sucesso em `{}`.",
                                            path.display()
                                        );
                                    }
                                    Err(err) => {
                                        self.error_banner = Some(format!(
                                            "Não foi possível salvar o arquivo `.lut`: {err}"
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("Alterar Pasta...")
                            .on_hover_text("Selecionar manualmente a pasta `Documents\\Assetto Corsa\\cfg`")
                            .clicked()
                        {
                            if let Some(folder) = rfd::FileDialog::new()
                                .set_title("Selecionar pasta `Assetto Corsa\\cfg`")
                                .pick_folder()
                            {
                                self.ac_cfg_dir = folder;
                            }
                        }
                        ui.label(
                            RichText::new(format!("Assetto Corsa: {}", self.ac_cfg_dir.display()))
                                .size(11.5_f32)
                                .color(COLOR_TEXT_MUTED),
                        );
                    });
                });
            });
    }
}

impl eframe::App for SpeedPreciseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Global Safety Hotkeys (`AGENTS.md` Rule 3): Space or ESC immediately cuts FFB motor torque
        let emergency_hotkey_pressed = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || (self.ffb_running && i.key_pressed(egui::Key::Space))
        });
        if emergency_hotkey_pressed && self.ffb_running {
            self.trigger_emergency_stop();
        }

        // 2. Non-blocking drain of events streamed from the background DirectInput8 worker
        self.pump_worker_events();

        // 3. Render Clean Header, Status Bar & Active Screen
        self.render_top_header(ctx);
        self.render_bottom_status_bar(ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(COLOR_BG_ROOT)
                    .inner_margin(14.0_f32),
            )
            .show(ctx, |ui| match self.active_screen {
                ActiveScreen::CockpitDashboard => self.render_screen_cockpit(ui),
                ActiveScreen::AutoLutStudio => self.render_screen_autolut(ui),
            });
    }
}

// ============================================================================
// Custom Cockpit Telemetry Painters & Clean Formatting Helpers
// ============================================================================

fn format_clean_device_label(dev: &DeviceInfo) -> String {
    if dev.has_ffb {
        format!("{} (FFB)", dev.instance_name)
    } else {
        dev.instance_name.clone()
    }
}

fn clean_axis_display_name(raw_name: &str, standard_label: &str) -> String {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        standard_label.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Paints a sleek horizontal center-zero steering bar (`0°` at center, `-half_rot°` Left to `+half_rot°` Right).
fn paint_horizontal_steering_gauge(ui: &mut egui::Ui, signed_pct: f64, wheel_rotation_deg: f64) {
    let desired_size = Vec2::new(ui.available_width().max(200.0_f32), 50.0_f32);
    let (outer_rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());
    let painter = ui.painter_at(outer_rect);

    let bar_rect = Rect::from_min_max(
        Pos2::new(outer_rect.min.x, outer_rect.min.y + 2.0_f32),
        Pos2::new(outer_rect.max.x, outer_rect.min.y + 30.0_f32),
    );

    // Track background
    painter.rect(
        bar_rect,
        Rounding::same(6.0_f32),
        COLOR_BG_TRACK,
        Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE),
    );

    let center_x = bar_rect.center().x;
    let half_w = bar_rect.width() * 0.5_f32;
    let norm = ((signed_pct / 100.0).clamp(-1.0, 1.0)) as f32;
    let needle_x = (center_x + norm * half_w).clamp(bar_rect.left() + 2.0_f32, bar_rect.right() - 2.0_f32);

    // Bidirectional fill from center `0°`
    if norm.abs() > 0.002_f32 {
        let (x_min, x_max) = if norm < 0.0_f32 {
            (needle_x, center_x)
        } else {
            (center_x, needle_x)
        };
        let fill_rect = Rect::from_min_max(
            Pos2::new(x_min, bar_rect.top() + 2.0_f32),
            Pos2::new(x_max, bar_rect.bottom() - 2.0_f32),
        );
        painter.rect_filled(fill_rect, Rounding::same(4.0_f32), COLOR_STEERING_FILL);
    }

    // Quarter & center grid ticks
    for frac in [-0.5_f32, 0.0_f32, 0.5_f32] {
        let tx = center_x + frac * half_w;
        let is_zero = frac.abs() < 0.01_f32;
        painter.line_segment(
            [Pos2::new(tx, bar_rect.top()), Pos2::new(tx, bar_rect.bottom())],
            Stroke::new(
                if is_zero { 2.0_f32 } else { 1.0_f32 },
                if is_zero {
                    Color32::from_rgb(232, 232, 232)
                } else {
                    COLOR_BORDER_SUBTLE
                },
            ),
        );
    }

    // Live steering position needle
    painter.line_segment(
        [
            Pos2::new(needle_x, bar_rect.top() - 1.0_f32),
            Pos2::new(needle_x, bar_rect.bottom() + 1.0_f32),
        ],
        Stroke::new(3.0_f32, Color32::WHITE),
    );

    // Scale labels (`-450°`, `0°`, `+450°`)
    let half_deg = (wheel_rotation_deg * 0.5).round() as i32;
    let label_y = bar_rect.bottom() + 4.0_f32;
    let font = FontId::proportional(11.0_f32);

    painter.text(
        Pos2::new(bar_rect.left() + 2.0_f32, label_y),
        Align2::LEFT_TOP,
        format!("-{half_deg}°"),
        font.clone(),
        COLOR_TEXT_MUTED,
    );
    painter.text(
        Pos2::new(center_x, label_y),
        Align2::CENTER_TOP,
        "0°",
        font.clone(),
        Color32::WHITE,
    );
    painter.text(
        Pos2::new(bar_rect.right() - 2.0_f32, label_y),
        Align2::RIGHT_TOP,
        format!("+{half_deg}°"),
        font,
        COLOR_TEXT_MUTED,
    );
}

/// Paints a cockpit-style vertical dual-bar pedal gauge (`0%` at bottom -> `100%` at top):
/// - Thin left bar: physical sensor travel (`0%..100%`) + saturation/deadzone markers.
/// - Wide right bar: final calibrated in-game output (`0%..100%`).
fn paint_vertical_pedal_bars(
    ui: &mut egui::Ui,
    height: f32,
    physical_pct: f64,
    ingame_pct: f64,
    saturation_pct: f64,
    deadzone_pct: f64,
    accent: Color32,
) {
    let desired_size = Vec2::new(ui.available_width().max(110.0_f32), height);
    let (area, _) = ui.allocate_exact_size(desired_size, Sense::hover());
    let painter = ui.painter_at(area);

    let top_y = area.top() + 6.0_f32;
    let bot_y = area.bottom() - 6.0_f32;
    let usable_h = (bot_y - top_y).max(40.0_f32);

    let raw_w = 14.0_f32;
    let gap = 10.0_f32;
    let main_w = 52.0_f32;
    let group_w = raw_w + gap + main_w;
    let start_x = area.center().x - group_w * 0.5_f32 - 8.0_f32;

    let raw_rect = Rect::from_min_max(
        Pos2::new(start_x, top_y),
        Pos2::new(start_x + raw_w, bot_y),
    );
    let main_rect = Rect::from_min_max(
        Pos2::new(raw_rect.right() + gap, top_y),
        Pos2::new(raw_rect.right() + gap + main_w, bot_y),
    );

    // Track backgrounds
    painter.rect(
        raw_rect,
        Rounding::same(4.0_f32),
        COLOR_BG_TRACK,
        Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE),
    );
    painter.rect(
        main_rect,
        Rounding::same(6.0_f32),
        COLOR_BG_TRACK,
        Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE),
    );

    // Bottom-up fill for physical raw travel
    let raw_frac = ((physical_pct / 100.0).clamp(0.0, 1.0)) as f32;
    if raw_frac > 0.002_f32 {
        let fill_top = bot_y - raw_frac * usable_h;
        let fill = Rect::from_min_max(
            Pos2::new(raw_rect.left() + 1.5_f32, fill_top),
            Pos2::new(raw_rect.right() - 1.5_f32, bot_y - 1.5_f32),
        );
        painter.rect_filled(fill, Rounding::same(3.0_f32), COLOR_PHYSICAL_RAW);
    }

    // Saturation (`S%`) & Deadzone (`D%`) threshold markers on the physical sensor bar
    let sat_frac = ((saturation_pct / 100.0).clamp(0.10, 1.0)) as f32;
    let sat_y = bot_y - sat_frac * usable_h;
    painter.line_segment(
        [
            Pos2::new(raw_rect.left() - 2.0_f32, sat_y),
            Pos2::new(raw_rect.right() + 2.0_f32, sat_y),
        ],
        Stroke::new(2.0_f32, COLOR_BADGE_WARN),
    );

    if deadzone_pct > 0.5 {
        let dz_frac = ((deadzone_pct / 100.0).clamp(0.0, 0.30)) as f32;
        let dz_y = bot_y - dz_frac * usable_h;
        painter.line_segment(
            [
                Pos2::new(raw_rect.left() - 2.0_f32, dz_y),
                Pos2::new(raw_rect.right() + 2.0_f32, dz_y),
            ],
            Stroke::new(1.5_f32, COLOR_TEXT_MUTED),
        );
    }

    // Bottom-up fill for In-Game calibrated output (`0%` bottom -> `100%` top)
    let game_frac = ((ingame_pct / 100.0).clamp(0.0, 1.0)) as f32;
    if game_frac > 0.002_f32 {
        let fill_top = bot_y - game_frac * usable_h;
        let fill = Rect::from_min_max(
            Pos2::new(main_rect.left() + 2.0_f32, fill_top),
            Pos2::new(main_rect.right() - 2.0_f32, bot_y - 2.0_f32),
        );
        painter.rect_filled(fill, Rounding::same(4.5_f32), accent);
    }

    // Horizontal scale ticks (`0%`, `25%`, `50%`, `75%`, `100%`) along main bar
    let font = FontId::proportional(10.5_f32);
    for pct in [0_i32, 25, 50, 75, 100] {
        let ty = bot_y - (pct as f32 / 100.0_f32) * usable_h;
        if pct > 0 && pct < 100 {
            painter.line_segment(
                [
                    Pos2::new(main_rect.left() + 3.0_f32, ty),
                    Pos2::new(main_rect.right() - 3.0_f32, ty),
                ],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 26)),
            );
        }
        if pct == 0 || pct == 50 || pct == 100 {
            painter.text(
                Pos2::new(main_rect.right() + 6.0_f32, ty),
                Align2::LEFT_CENTER,
                format!("{pct}%"),
                font.clone(),
                COLOR_TEXT_MUTED,
            );
        }
    }
}

// ============================================================================
// Crisp Vector UI Icons (`egui::Painter`) — Zero Missing Font Glyphs (`□`)
// ============================================================================

/// Vector icon variants drawn directly with anti-aliased `egui::Painter` primitives,
/// eliminating missing-glyph boxes (`U+25CF`, `U+2713`, `U+27F3`) and `NotoEmoji` baseline drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VectorIcon {
    Refresh,
    Check,
    Play,
    Folder,
    Stop,
    ResetZoom,
    ApplyCheck,
    Save,
}

/// Paints a crisp status indicator LED aligned vertically with status bar text.
fn paint_status_led(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0_f32, 16.0_f32), Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    painter.circle_filled(c, 5.5_f32, color.gamma_multiply(0.22_f32));
    painter.circle_filled(c, 3.6_f32, color);
}

/// Paints a crisp vector warning triangle (`!`) aligned vertically with error banner text.
fn paint_warning_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0_f32, 16.0_f32), Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let top = Pos2::new(c.x, c.y - 6.0_f32);
    let bl = Pos2::new(c.x - 6.6_f32, c.y + 5.4_f32);
    let br = Pos2::new(c.x + 6.6_f32, c.y + 5.4_f32);
    painter.add(egui::Shape::closed_line(
        vec![top, br, bl],
        Stroke::new(1.5_f32, color),
    ));
    painter.line_segment(
        [Pos2::new(c.x, c.y - 2.2_f32), Pos2::new(c.x, c.y + 1.5_f32)],
        Stroke::new(1.6_f32, color),
    );
    painter.circle_filled(Pos2::new(c.x, c.y + 3.6_f32), 0.9_f32, color);
}

/// Paints one of the 8 UI vector icons inside `rect` with exact pixel-centered proportions.
fn paint_vector_icon(painter: &egui::Painter, rect: Rect, icon: VectorIcon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5_f32;

    match icon {
        VectorIcon::Refresh => {
            let arc_r = r * 0.76_f32;
            let start_deg = 35.0_f32;
            let end_deg = 305.0_f32;
            let steps = 14_usize;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let ang = (start_deg + t * (end_deg - start_deg)).to_radians();
                pts.push(Pos2::new(
                    c.x + arc_r * ang.cos(),
                    c.y + arc_r * ang.sin(),
                ));
            }
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.55_f32, color)));

            if let Some(&tip) = pts.last() {
                let end_rad = end_deg.to_radians();
                // Tangent direction along clockwise sweep at `end_deg`
                let tx = -end_rad.sin();
                let ty = end_rad.cos();
                let nx = -end_rad.cos();
                let ny = -end_rad.sin();
                let wing = r * 0.42_f32;
                let p_left = Pos2::new(
                    tip.x - tx * wing + nx * (wing * 0.65_f32),
                    tip.y - ty * wing + ny * (wing * 0.65_f32),
                );
                let p_right = Pos2::new(
                    tip.x - tx * wing - nx * (wing * 0.55_f32),
                    tip.y - ty * wing - ny * (wing * 0.55_f32),
                );
                painter.add(egui::Shape::line(
                    vec![p_left, tip, p_right],
                    Stroke::new(1.55_f32, color),
                ));
            }
        }
        VectorIcon::Check => {
            let p1 = Pos2::new(c.x - r * 0.72_f32, c.y + r * 0.04_f32);
            let p2 = Pos2::new(c.x - r * 0.18_f32, c.y + r * 0.58_f32);
            let p3 = Pos2::new(c.x + r * 0.78_f32, c.y - r * 0.48_f32);
            painter.add(egui::Shape::line(
                vec![p1, p2, p3],
                Stroke::new(1.85_f32, color),
            ));
        }
        VectorIcon::Play => {
            let ox = c.x + r * 0.08_f32;
            let p1 = Pos2::new(ox - r * 0.56_f32, c.y - r * 0.68_f32);
            let p2 = Pos2::new(ox + r * 0.68_f32, c.y);
            let p3 = Pos2::new(ox - r * 0.56_f32, c.y + r * 0.68_f32);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                Stroke::NONE,
            ));
        }
        VectorIcon::Folder => {
            let left = c.x - r * 0.84_f32;
            let right = c.x + r * 0.84_f32;
            let top_tab = c.y - r * 0.64_f32;
            let top_body = c.y - r * 0.36_f32;
            let bot = c.y + r * 0.68_f32;
            let tab_mid = c.x - r * 0.16_f32;
            let tab_slope = c.x + r * 0.06_f32;

            painter.add(egui::Shape::closed_line(
                vec![
                    Pos2::new(left, top_tab),
                    Pos2::new(tab_mid, top_tab),
                    Pos2::new(tab_slope, top_body),
                    Pos2::new(right, top_body),
                    Pos2::new(right, bot),
                    Pos2::new(left, bot),
                ],
                Stroke::new(1.5_f32, color),
            ));
        }
        VectorIcon::Stop => {
            painter.circle_stroke(c, r * 0.86_f32, Stroke::new(1.6_f32, color));
            let sq = Rect::from_center_size(c, Vec2::splat(r * 0.78_f32));
            painter.rect_filled(sq, Rounding::same(1.2_f32), color);
        }
        VectorIcon::ResetZoom => {
            let lens_c = Pos2::new(c.x - r * 0.14_f32, c.y - r * 0.14_f32);
            let lens_r = r * 0.54_f32;
            painter.circle_stroke(lens_c, lens_r, Stroke::new(1.55_f32, color));
            let d = lens_r * 0.7071_f32;
            painter.line_segment(
                [
                    Pos2::new(lens_c.x + d, lens_c.y + d),
                    Pos2::new(c.x + r * 0.80_f32, c.y + r * 0.80_f32),
                ],
                Stroke::new(1.8_f32, color),
            );
            painter.line_segment(
                [
                    Pos2::new(lens_c.x - lens_r * 0.45_f32, lens_c.y),
                    Pos2::new(lens_c.x + lens_r * 0.45_f32, lens_c.y),
                ],
                Stroke::new(1.3_f32, color),
            );
        }
        VectorIcon::ApplyCheck => {
            painter.circle_stroke(c, r * 0.88_f32, Stroke::new(1.5_f32, color));
            let p1 = Pos2::new(c.x - r * 0.44_f32, c.y + r * 0.04_f32);
            let p2 = Pos2::new(c.x - r * 0.10_f32, c.y + r * 0.38_f32);
            let p3 = Pos2::new(c.x + r * 0.48_f32, c.y - r * 0.28_f32);
            painter.add(egui::Shape::line(
                vec![p1, p2, p3],
                Stroke::new(1.65_f32, color),
            ));
        }
        VectorIcon::Save => {
            // Downward arrow into tray
            let arrow_top = Pos2::new(c.x, c.y - r * 0.74_f32);
            let arrow_tip = Pos2::new(c.x, c.y + r * 0.24_f32);
            painter.line_segment([arrow_top, arrow_tip], Stroke::new(1.65_f32, color));
            painter.add(egui::Shape::line(
                vec![
                    Pos2::new(c.x - r * 0.42_f32, c.y - r * 0.16_f32),
                    arrow_tip,
                    Pos2::new(c.x + r * 0.42_f32, c.y - r * 0.16_f32),
                ],
                Stroke::new(1.65_f32, color),
            ));
            let tray_l = c.x - r * 0.76_f32;
            let tray_r = c.x + r * 0.76_f32;
            let tray_top = c.y + r * 0.26_f32;
            let tray_bot = c.y + r * 0.74_f32;
            painter.add(egui::Shape::line(
                vec![
                    Pos2::new(tray_l, tray_top),
                    Pos2::new(tray_l, tray_bot),
                    Pos2::new(tray_r, tray_bot),
                    Pos2::new(tray_r, tray_top),
                ],
                Stroke::new(1.55_f32, color),
            ));
        }
    }
}

/// Renders a button with a pixel-aligned vector icon on the left and strong label text on the right.
#[allow(clippy::too_many_arguments)]
fn add_icon_button(
    ui: &mut egui::Ui,
    icon: VectorIcon,
    label: &str,
    font_size: f32,
    min_size: Vec2,
    custom_fill: Option<Color32>,
    custom_stroke: Option<Stroke>,
    rounding: f32,
) -> egui::Response {
    let enabled = ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };

    let icon_dim = 13.0_f32;
    let gap = 7.0_f32;
    let font_id = FontId::proportional(font_size);

    let measure_galley = ui.fonts(|f| {
        f.layout_no_wrap(label.to_string(), font_id.clone(), Color32::WHITE)
    });
    let desired_size = Vec2::new(
        min_size
            .x
            .max(icon_dim + gap + measure_galley.size().x + 20.0_f32),
        min_size.y.max(measure_galley.size().y + 10.0_f32),
    );

    let (rect, response) = ui.allocate_exact_size(desired_size, sense);

    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let (bg_fill, border_stroke, fg_color) = if !enabled {
        (
            COLOR_BG_CARD,
            Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE),
            Color32::from_rgb(112, 112, 112),
        )
    } else if response.is_pointer_button_down_on() {
        let fill = match custom_fill {
            Some(c) => c.gamma_multiply(0.84_f32),
            None => ui.visuals().widgets.active.bg_fill,
        };
        let stroke = custom_stroke.unwrap_or(ui.visuals().widgets.active.bg_stroke);
        (fill, stroke, Color32::WHITE)
    } else if response.hovered() {
        let fill = match custom_fill {
            Some(c) => Color32::from_rgb(
                c.r().saturating_add(20),
                c.g().saturating_add(20),
                c.b().saturating_add(20),
            ),
            None => ui.visuals().widgets.hovered.bg_fill,
        };
        let stroke = match custom_stroke {
            Some(s) => Stroke::new(s.width, Color32::from_rgb(200, 200, 200)),
            None => ui.visuals().widgets.hovered.bg_stroke,
        };
        (fill, stroke, Color32::WHITE)
    } else {
        let fill = custom_fill.unwrap_or(ui.visuals().widgets.inactive.bg_fill);
        let stroke = custom_stroke.unwrap_or(ui.visuals().widgets.inactive.bg_stroke);
        let fg = if custom_fill.is_some() {
            Color32::WHITE
        } else {
            COLOR_TEXT_PRIMARY
        };
        (fill, stroke, fg)
    };

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(rect, Rounding::same(rounding), bg_fill, border_stroke);

        let galley = painter.layout_no_wrap(label.to_string(), font_id, fg_color);
        let total_w = icon_dim + gap + galley.size().x;
        let start_x = (rect.center().x - total_w * 0.5_f32).max(rect.left() + 8.0_f32);

        let icon_rect = Rect::from_center_size(
            Pos2::new(start_x + icon_dim * 0.5_f32, rect.center().y),
            Vec2::splat(icon_dim),
        );
        paint_vector_icon(painter, icon_rect, icon, fg_color);

        let text_pos = Pos2::new(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y * 0.5_f32,
        );
        painter.galley(text_pos, galley, fg_color);
    }

    response
}

fn configure_cockpit_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.window_fill = COLOR_BG_PANEL;
    visuals.panel_fill = COLOR_BG_PANEL;
    visuals.faint_bg_color = COLOR_BG_CARD;
    visuals.extreme_bg_color = COLOR_BG_TRACK;
    visuals.code_bg_color = Color32::from_rgb(24, 24, 24);
    visuals.window_stroke = Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE);

    // Non-interactive frames, separators & labels
    visuals.widgets.noninteractive.bg_fill = COLOR_BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = COLOR_BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, COLOR_BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, COLOR_TEXT_PRIMARY);

    // Resting buttons, sliders, combo boxes & drag values (neutral dark-gray, zero blue tint)
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(48, 48, 48);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(45, 45, 45);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(74, 74, 74));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, COLOR_TEXT_PRIMARY);

    // Hovered interactive controls
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(64, 64, 64);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 60, 60);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(116, 116, 116));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);

    // Active / pressed interactive controls & slider fill
    visuals.widgets.active.bg_fill = Color32::from_rgb(82, 82, 82);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(78, 78, 78);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(160, 160, 160));
    visuals.widgets.active.fg_stroke = Stroke::new(2.0_f32, Color32::WHITE);

    // Open ComboBox / menu state
    visuals.widgets.open.bg_fill = Color32::from_rgb(58, 58, 58);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(56, 56, 56);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(124, 124, 124));
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);

    // Selection & slider rail highlight
    visuals.selection.bg_fill = Color32::from_rgb(78, 78, 78);
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = COLOR_TEXT_SECONDARY;

    ctx.set_visuals(visuals);
}

#[cfg(test)]
mod tests {
    use super::*;
    use speed_precise_core::AxisCalibration;
    use std::time::{Duration, Instant};

    #[test]
    fn test_autolut_unified_pipeline_and_contextual_emergency_stop() {
        let mut app = SpeedPreciseApp::new_headless_for_test(WorkerConfig {
            telemetry_poll_interval: Duration::from_millis(25),
            dry_run_step_delay: Duration::ZERO,
            enable_physical_dinput: false,
        });

        // Initially idle: contextual emergency stop button MUST be hidden
        assert!(!app.is_emergency_stop_visible());
        assert_eq!(app.active_screen, ActiveScreen::CockpitDashboard);

        app.step_dry_run = true;
        app.step_count = 20;
        app.launch_step_log2();

        // While running: switches to Auto-LUT screen and shows contextual emergency stop
        assert_eq!(app.active_screen, ActiveScreen::AutoLutStudio);
        assert!(app.is_emergency_stop_visible());

        let deadline = Instant::now() + Duration::from_secs(5);
        while app.ffb_running && Instant::now() < deadline {
            app.pump_worker_events();
            std::thread::sleep(Duration::from_millis(2));
        }

        // Once Step 20 completes: emergency stop hides AND final LUT is immediately populated on the same screen
        assert!(!app.ffb_running);
        assert!(!app.is_emergency_stop_visible());
        assert_eq!(app.step_samples.len(), 21);

        let lut = app
            .lut_result
            .as_ref()
            .expect("Unified Auto-LUT screen must compute final LUT immediately on completion");
        assert_eq!(lut.points.len(), 101);
        assert!(lut.deadzone_percent >= 10.0);
        assert!(lut.max_displacement_deg > 50.0);
    }

    #[test]
    fn test_app_emergency_stop_transitions_state_immediately() {
        let mut app = SpeedPreciseApp::new_headless_for_test(WorkerConfig {
            telemetry_poll_interval: Duration::from_millis(25),
            dry_run_step_delay: Duration::from_millis(20),
            enable_physical_dinput: false,
        });

        app.step_dry_run = true;
        app.step_count = 50;
        app.launch_step_log2();
        assert!(app.is_emergency_stop_visible());

        app.trigger_emergency_stop();
        assert!(!app.ffb_running);
        assert!(!app.is_emergency_stop_visible());
        assert!(app.step_state_badge.contains("Interrompido"));
    }

    #[test]
    fn test_lut_plot_bounds_strictly_clamped_to_unit_square() {
        // 1. Attempting to zoom out beyond [0.0, 1.0] locks strictly to [0.0, 1.0] x [0.0, 1.0]
        let (min_out, max_out) =
            zoom_unit_square_bounds([0.0, 0.0], [1.0, 1.0], 0.5, [0.5, 0.5]);
        assert_eq!(min_out, [0.0, 0.0]);
        assert_eq!(max_out, [1.0, 1.0]);

        // 2. Zooming in 2x around (0.2, 0.2) produces a 0.5 x 0.5 window strictly >= 0.0
        let (min_in, max_in) =
            zoom_unit_square_bounds([0.0, 0.0], [1.0, 1.0], 2.0, [0.2, 0.2]);
        assert!(min_in[0] >= 0.0 && min_in[1] >= 0.0);
        assert!(max_in[0] <= 1.0 && max_in[1] <= 1.0);
        assert!(((max_in[0] - min_in[0]) - 0.5).abs() < 1e-9);

        // 3. Attempting to pan outside negative (< 0.0) or (> 1.0) clamps against wall while preserving span
        let (clamped_lo, clamped_hi) = clamp_unit_square_bounds([-0.25, 0.80], [0.15, 1.20]);
        assert!((clamped_lo[0] - 0.0).abs() < 1e-9);
        assert!((clamped_hi[0] - 0.40).abs() < 1e-9);
        assert!((clamped_lo[1] - 0.60).abs() < 1e-9);
        assert!((clamped_hi[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_pedal_output_updates_across_saturation_and_deadzone_without_50pct_lock() {
        // Simulates a Logitech G920 Brake axis (`0..=255`, inverted: rest = 255, full press = 0)
        // where `dinput8.dll` reset `cooked_value = 32767` (`cooked_percent = 50.0%`) after `SetProperty`,
        // EVEN when `snap.calibration` already matches the newly-written `[lMin, lCenter, lMax]`.
        let make_snap = |raw_val: i32, active_cal: AxisCalibration| AxisTelemetrySnapshot {
            axis_index: 2,
            dw_type: 0x0000_0502,
            instance_id: 5,
            name: "Brake".to_string(),
            standard_label: "Brake / Rz",
            raw_value: raw_val,
            cooked_value: 32_767, // Synthetic 50.0% reset value emitted by `dinput8.dll` after `SetProperty`
            logical_range: (0, 255),
            output_range: (0, 65_535),
            calibration: active_cal,
            deadzone: 0,
            saturation: 10_000,
            cooked_percent: 50.0,
        };

        // Case 1: Saturation reduced to 40% (`[153, 204, 255]`), Deadzone = 0%
        let cal_sat_40 = compute_smart_pedal_calibration(0, 255, 40.0, 0.0, true).unwrap();
        // - Foot off (`raw = 255`): Output MUST be `0%` (NOT stuck at `50%`!)
        let (phys_rest, out_rest) = compute_live_pedal_display_percentages(
            &make_snap(255, cal_sat_40),
            40.0,
            0.0,
            true,
        );
        assert!((phys_rest - 0.0).abs() < 1e-6);
        assert!(
            out_rest.abs() < 1e-6,
            "Expected 0% output at rest after saturation change, got {out_rest:.2}%"
        );

        // - Halfway into 40% saturation (`20%` physical press -> `raw = 204`): Output = `50%`
        let (phys_mid, out_mid) = compute_live_pedal_display_percentages(
            &make_snap(204, cal_sat_40),
            40.0,
            0.0,
            true,
        );
        assert!((phys_mid - 20.0).abs() < 0.5);
        assert!((out_mid - 50.0).abs() < 1.0);

        // - Reaching 40% physical travel (`raw = 153`) and beyond (`raw = 90`): Output = `100%`
        let (_, out_sat) = compute_live_pedal_display_percentages(
            &make_snap(153, cal_sat_40),
            40.0,
            0.0,
            true,
        );
        assert!((out_sat - 100.0).abs() < 1e-6);

        let (_, out_beyond) = compute_live_pedal_display_percentages(
            &make_snap(90, cal_sat_40),
            40.0,
            0.0,
            true,
        );
        assert!((out_beyond - 100.0).abs() < 1e-6);

        // Case 2: Deadzone raised to 30% (`[0, 89, 179]`), Saturation = 100%
        let cal_dz_30 = compute_smart_pedal_calibration(0, 255, 100.0, 30.0, true).unwrap();
        // - Inside 30% deadzone (`raw = 255` rest and `raw = 200` ~ 21.5% press): Output MUST stay `0%`
        let (_, out_dz_rest) = compute_live_pedal_display_percentages(
            &make_snap(255, cal_dz_30),
            100.0,
            30.0,
            true,
        );
        assert!(out_dz_rest.abs() < 1e-6);

        let (_, out_dz_inside) = compute_live_pedal_display_percentages(
            &make_snap(200, cal_dz_30),
            100.0,
            30.0,
            true,
        );
        assert!(
            out_dz_inside.abs() < 1e-6,
            "Expected 0% inside 30% deadzone, got {out_dz_inside:.2}%"
        );

        // - Midpoint of active window (`raw = 89` ~ 65% press): Output ~ `50%`
        let (_, out_dz_mid) = compute_live_pedal_display_percentages(
            &make_snap(89, cal_dz_30),
            100.0,
            30.0,
            true,
        );
        assert!((out_dz_mid - 50.0).abs() < 1.5);

        // - Full press (`raw = 0`): Output = `100%`
        let (_, out_dz_full) = compute_live_pedal_display_percentages(
            &make_snap(0, cal_dz_30),
            100.0,
            30.0,
            true,
        );
        assert!((out_dz_full - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_dark_theme_palette_is_neutral_gray_without_blue_cast() {
        let neutral_surfaces_and_texts = [
            ("COLOR_BG_ROOT", COLOR_BG_ROOT),
            ("COLOR_BG_PANEL", COLOR_BG_PANEL),
            ("COLOR_BG_CARD", COLOR_BG_CARD),
            ("COLOR_BG_TRACK", COLOR_BG_TRACK),
            ("COLOR_BORDER_SUBTLE", COLOR_BORDER_SUBTLE),
            ("COLOR_TEXT_PRIMARY", COLOR_TEXT_PRIMARY),
            ("COLOR_TEXT_SECONDARY", COLOR_TEXT_SECONDARY),
            ("COLOR_TEXT_MUTED", COLOR_TEXT_MUTED),
            ("COLOR_ACCENT_SURFACE", COLOR_ACCENT_SURFACE),
            ("COLOR_ACCENT_BORDER", COLOR_ACCENT_BORDER),
            ("COLOR_STEERING_FILL", COLOR_STEERING_FILL),
            ("COLOR_PHYSICAL_RAW", COLOR_PHYSICAL_RAW),
            ("COLOR_PLOT_IDEAL", COLOR_PLOT_IDEAL),
        ];

        for (name, c) in neutral_surfaces_and_texts {
            assert!(
                c.r() == c.g() && c.g() == c.b(),
                "{name} must be neutral gray without blue cast, got RGB({}, {}, {})",
                c.r(),
                c.g(),
                c.b()
            );
        }

        // Verify comfortable luminosity and contrast hierarchy (not pitch-black or unreadable)
        assert!(COLOR_BG_ROOT.r() >= 18 && COLOR_BG_ROOT.r() <= 28);
        assert!(COLOR_BG_CARD.r() >= 30 && COLOR_BG_CARD.r() <= 44);
        assert!(COLOR_TEXT_PRIMARY.r() >= 235);
        assert!(COLOR_TEXT_MUTED.r() >= 150);
    }

    #[test]
    fn test_steering_gauge_updates_live_when_cooked_stuck_at_50pct() {
        let make_wheel_snap = |raw_val: i32, cooked_pct: f64| AxisTelemetrySnapshot {
            axis_index: 0,
            dw_type: 0x0100_0002,
            instance_id: 0,
            name: "Wheel axis".to_string(),
            standard_label: "X Axis",
            raw_value: raw_val,
            cooked_value: 32_767,
            logical_range: (0, 65_535),
            output_range: (0, 65_535),
            calibration: AxisCalibration::default_reset(0, 65_535),
            deadzone: 0,
            saturation: 10_000,
            cooked_percent: cooked_pct,
        };

        // When DirectInput's cooked buffer is stuck at 50.0% (`32767`), live angle bar must still track raw travel
        for (raw, expected_signed_pct) in [
            (0_i32, -100.0_f64),
            (16_384_i32, -50.0_f64),
            (32_768_i32, 0.0_f64),
            (49_152_i32, 50.0_f64),
            (65_535_i32, 100.0_f64),
        ] {
            let snap = make_wheel_snap(raw, 50.0);
            for preset in [540.0_f64, 900.0_f64, 1080.0_f64] {
                let (angle_deg, signed_pct) = compute_live_steering_angle_and_pct(Some(&snap), preset);
                let expected_angle = (expected_signed_pct / 100.0) * (preset * 0.5);
                assert!(
                    (signed_pct - expected_signed_pct).abs() < 0.1,
                    "raw={raw} preset={preset}: expected signed_pct={expected_signed_pct}, got {signed_pct}"
                );
                assert!(
                    (angle_deg - expected_angle).abs() < 0.25,
                    "raw={raw} preset={preset}: expected angle={expected_angle}, got {angle_deg}"
                );
            }
        }

        // Conversely, if only cooked telemetry moves while raw sits at logical midpoint, honor cooked telemetry
        let cooked_driven = make_wheel_snap(32_768, 25.0);
        let (angle_cooked, pct_cooked) =
            compute_live_steering_angle_and_pct(Some(&cooked_driven), 900.0);
        assert!((pct_cooked + 50.0).abs() < 0.1);
        assert!((angle_cooked + 225.0).abs() < 0.25);

        // And with no snapshot yet, default safely to center 0°
        let (angle_none, pct_none) = compute_live_steering_angle_and_pct(None, 900.0);
        assert!(angle_none.abs() < 1e-6 && pct_none.abs() < 1e-6);
    }
}
