//! Multi-threaded DirectInput8 hardware worker (`worker.rs`).
//!
//! Strictly decouples all Win32 / `dinput8.dll` COM polling (`60 Hz` dual-handle telemetry)
//! and the real-time `1000 Hz` (`~58s`) `Step Log 2` Force Feedback loop from the `eframe`/`egui`
//! UI rendering thread via `std::sync::mpsc` channels and a zero-latency `Arc<AtomicBool>`
//! emergency cutoff flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use speed_precise_core::{
    compute_step_force, run_step_log2_with_backend, AxisCalibration, AxisTelemetrySnapshot,
    ControllerDeviceInfo, CoreError, CoreResult, DirectInputContext, DualDeviceHandle,
    FfbHardwareBackend, FfbSession, LiveCalibrationOutcome, StepLog2Config, StepLogSample,
    VirtualWheelSimulator, DEFAULT_STEP_LOG2_SETTLE_MS,
};

/// Alias matching the GUI worker event contract for discovered DirectInput8 controllers.
pub type DeviceInfo = ControllerDeviceInfo;

/// Commands dispatched asynchronously from `SpeedPreciseApp` (UI thread) to the background
/// DirectInput8 hardware worker thread.
#[derive(Debug, Clone)]
pub enum GuiCommand {
    /// Re-enumerates connected USB DirectInput8 game controllers.
    RefreshDevices,
    /// Opens DIView's simultaneous Raw + Cooked dual handles (`pRawDevice` + `pCookedDevice`)
    /// on the selected controller index and starts `~60 Hz` live axis telemetry streaming.
    SelectDevice(usize),
    /// Writes live 3-point calibration (`DIPROP_CALIBRATION = 0x0B`), deadzone, and saturation
    /// to Windows Registry/driver after persisting a JSON backup in `%APPDATA%\SpeedPrecise\backups\`.
    ApplyAxisCalibration {
        device_idx: usize,
        axis_id: u32,
        min: i32,
        center: i32,
        max: i32,
        deadzone: Option<u32>,
        saturation: Option<u32>,
    },
    /// Restores DIView factory logical bounds `[lMin, lCenter, lMax]`, `deadzone = 0`,
    /// and `saturation = 10000` on the target axis (with automatic JSON backup).
    RestoreDefaultCalibration {
        device_idx: usize,
        axis_id: u32,
    },
    /// Launches the `WheelCheck` `Step Log 2` linearity test at `1000 Hz` on physical hardware
    /// (acquiring `DISCL_EXCLUSIVE`) or on the deterministic `VirtualWheelSimulator` (`dry_run`).
    StartStepLog2 {
        device_idx: usize,
        steps: u32,
        duration_ms: u64,
        max_angle_deg: f64,
        max_force: i32,
        dry_run: bool,
    },
    /// Triggers immediate Force Feedback cutoff (`torque = 0`, `IDirectInputEffect::Stop`, `Unacquire`).
    EmergencyStopFfb,
    /// Cleanly terminates the worker thread when the application closes.
    Shutdown,
}

/// Asynchronous events streamed from the background hardware worker thread to `SpeedPreciseApp`.
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    /// Updated snapshot of all connected DirectInput8 controllers and their axes.
    DeviceListUpdated(Vec<DeviceInfo>),
    /// Simultaneous Raw + Calibrated (`Cooked`) telemetry for all axes of the selected device.
    LiveAxisTelemetry(Vec<AxisTelemetrySnapshot>),
    /// Confirmation and readback verification of a live `DIPROP_CALIBRATION` (`0x0B`) update.
    CalibrationApplied(LiveCalibrationOutcome),
    /// Intra-step live state machine transition (`"PID Centering"`, `"Settle 300ms"`, `"Force Pulse"`).
    StepLog2StateChanged {
        current_step: u32,
        total_steps: u32,
        state_label: String,
    },
    /// Emitted immediately as each `Step Log 2` force pulse completes (`1..=total_steps`).
    StepLog2Progress {
        current_step: u32,
        total_steps: u32,
        sample: StepLogSample,
        state_label: String,
    },
    /// Emitted when all `Step Log 2` steps finish cleanly (`Row 0` + `1..=N` samples).
    StepLog2Finished(Vec<StepLogSample>),
    /// Emitted when `Step Log 2` is aborted via `EmergencyStopFfb` (`Space` / `ESC` / red button) or error.
    StepLog2Aborted,
    /// Human-readable status update for the bottom/header telemetry bar.
    StatusMessage(String),
    /// Non-fatal hardware or DirectInput8 error message surfaced in the UI.
    Error(String),
}

/// Timing and hardware-isolation parameters for [`HardwareWorker`].
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// Polling cadence for `DualDeviceHandle::poll_telemetry` while idle (`16 ms` ≈ `62.5 Hz`).
    pub telemetry_poll_interval: Duration,
    /// Optional visual pacing delay per step in `--dry-run` mode (`12 ms` in GUI so live plotting
    /// and emergency stop remain interactive; `0 ms` or `15 ms` in headless unit tests).
    pub dry_run_step_delay: Duration,
    /// Whether this worker instance opens physical `dinput8.dll` COM handles (`true` in production
    /// GUI; `false` in parallel headless dry-run unit tests to prevent concurrent multi-thread
    /// USB driver contention across parallel test threads).
    pub enable_physical_dinput: bool,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            telemetry_poll_interval: Duration::from_millis(16),
            dry_run_step_delay: Duration::from_millis(12),
            enable_physical_dinput: true,
        }
    }
}

/// Handle owned by `SpeedPreciseApp` on the UI thread to communicate with the isolated
/// DirectInput8 / Force Feedback worker thread without ever blocking frame rendering.
pub struct HardwareWorker {
    cmd_tx: Sender<GuiCommand>,
    evt_rx: Receiver<WorkerEvent>,
    ffb_abort_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl HardwareWorker {
    /// Spawns the dedicated DirectInput8 hardware worker thread with default production settings.
    pub fn spawn(egui_ctx: Option<egui::Context>) -> Self {
        Self::spawn_with_config(egui_ctx, WorkerConfig::default())
    }

    /// Spawns the dedicated DirectInput8 hardware worker thread with custom [`WorkerConfig`].
    pub fn spawn_with_config(egui_ctx: Option<egui::Context>, config: WorkerConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<GuiCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<WorkerEvent>();
        let ffb_abort_flag = Arc::new(AtomicBool::new(false));
        let worker_abort = Arc::clone(&ffb_abort_flag);

        let join_handle = thread::Builder::new()
            .name("speed-precise-hw-worker".to_string())
            .spawn(move || {
                worker_thread_main(cmd_rx, evt_tx, worker_abort, egui_ctx, config);
            })
            .expect("Failed to spawn `speed-precise-hw-worker` background thread");

        Self {
            cmd_tx,
            evt_rx,
            ffb_abort_flag,
            join_handle: Some(join_handle),
        }
    }

    /// Sends an asynchronous [`GuiCommand`] to the worker thread without blocking the UI.
    pub fn send(&self, cmd: GuiCommand) {
        if matches!(cmd, GuiCommand::EmergencyStopFfb) {
            self.ffb_abort_flag.store(true, Ordering::SeqCst);
        }
        let _ = self.cmd_tx.send(cmd);
    }

    /// Atomically resets the emergency abort flag and dispatches [`GuiCommand::StartStepLog2`].
    ///
    /// Clearing `ffb_abort_flag` on the caller side *before* enqueuing `StartStepLog2` guarantees
    /// that an `EmergencyStopFfb` triggered even microseconds later is never overwritten.
    pub fn start_step_log2(
        &self,
        device_idx: usize,
        steps: u32,
        duration_ms: u64,
        max_angle_deg: f64,
        max_force: i32,
        dry_run: bool,
    ) {
        self.ffb_abort_flag.store(false, Ordering::SeqCst);
        let _ = self.cmd_tx.send(GuiCommand::StartStepLog2 {
            device_idx,
            steps,
            duration_ms,
            max_angle_deg,
            max_force,
            dry_run,
        });
    }

    /// Triggers an immediate sub-millisecond Force Feedback motor cutoff (`< 1 ms` latency).
    ///
    /// Sets the shared `Arc<AtomicBool>` read on every `1000 Hz` tick of `run_step_log2_with_backend`
    /// AND sends [`GuiCommand::EmergencyStopFfb`] to cancel any queued command.
    pub fn trigger_emergency_stop(&self) {
        self.ffb_abort_flag.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(GuiCommand::EmergencyStopFfb);
    }

    /// Non-blocking poll of the next [`WorkerEvent`] queued for the UI thread.
    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.evt_rx.try_recv().ok()
    }

    /// Exposes a read-only reference to the shared atomic FFB abort flag.
    pub fn abort_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ffb_abort_flag)
    }
}

impl Drop for HardwareWorker {
    fn drop(&mut self) {
        self.ffb_abort_flag.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(GuiCommand::Shutdown);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// Transparent `FfbHardwareBackend` Phase Observer for Live GUI State Badges
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedFfbPhase {
    Init,
    BreakawayRamp,
    PidCentering,
    Settle300ms,
    ForcePulse,
}

impl ObservedFfbPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Init => "Preparando volante",
            Self::BreakawayRamp => "Alinhando centro",
            Self::PidCentering => "Centralizando volante",
            Self::Settle300ms => "Estabilizando",
            Self::ForcePulse => "Medindo resposta",
        }
    }
}

/// Zero-overhead decorator around any [`FfbHardwareBackend`] (`FfbSession` or `VirtualWheelSimulator`)
/// that detects `PID Centering` -> `Settle 300ms` -> `Force Pulse` state transitions without
/// altering the `1000 Hz` sub-millisecond timing loop in `speed-precise-core`.
struct GuiMonitoredBackend<'a, B: FfbHardwareBackend> {
    inner: &'a mut B,
    evt_tx: &'a Sender<WorkerEvent>,
    egui_ctx: &'a Option<egui::Context>,
    total_steps: u32,
    max_force: i32,
    active_step: u32,
    phase: ObservedFfbPhase,
    zero_force_ticks: u32,
}

impl<'a, B: FfbHardwareBackend> GuiMonitoredBackend<'a, B> {
    fn new(
        inner: &'a mut B,
        evt_tx: &'a Sender<WorkerEvent>,
        egui_ctx: &'a Option<egui::Context>,
        total_steps: u32,
        max_force: i32,
    ) -> Self {
        Self {
            inner,
            evt_tx,
            egui_ctx,
            total_steps,
            max_force,
            active_step: 1,
            phase: ObservedFfbPhase::Init,
            zero_force_ticks: 0,
        }
    }

    fn emit_phase_if_changed(&mut self, next_phase: ObservedFfbPhase) {
        if self.phase != next_phase {
            self.phase = next_phase;
            send_event(
                self.evt_tx,
                self.egui_ctx,
                WorkerEvent::StepLog2StateChanged {
                    current_step: self.active_step.min(self.total_steps),
                    total_steps: self.total_steps,
                    state_label: next_phase.label().to_string(),
                },
            );
        }
    }
}

impl<B: FfbHardwareBackend> FfbHardwareBackend for GuiMonitoredBackend<'_, B> {
    #[inline]
    fn logical_range(&self) -> (i32, i32) {
        self.inner.logical_range()
    }

    #[inline]
    fn read_raw_x(&mut self) -> CoreResult<i32> {
        self.inner.read_raw_x()
    }

    fn apply_force(&mut self, force: i32) -> CoreResult<()> {
        let expected_pulse =
            compute_step_force(self.active_step, self.total_steps, self.max_force, 1);

        if force == 0 {
            self.zero_force_ticks = self.zero_force_ticks.saturating_add(1);
            // State 3 (`MotorOffSettle`) holds `force = 0` continuously for `300 ms`
            if self.zero_force_ticks >= 8 && self.phase != ObservedFfbPhase::Init {
                self.emit_phase_if_changed(ObservedFfbPhase::Settle300ms);
            }
        } else {
            let came_from_settle =
                self.phase == ObservedFfbPhase::Settle300ms || self.zero_force_ticks >= 8;
            self.zero_force_ticks = 0;

            if came_from_settle && force == expected_pulse {
                self.emit_phase_if_changed(ObservedFfbPhase::ForcePulse);
                self.active_step = self.active_step.saturating_add(1);
            } else if self.phase == ObservedFfbPhase::Init && force < 0 {
                self.emit_phase_if_changed(ObservedFfbPhase::BreakawayRamp);
            } else if self.phase != ObservedFfbPhase::ForcePulse {
                self.emit_phase_if_changed(ObservedFfbPhase::PidCentering);
            }
        }

        self.inner.apply_force(force)
    }

    #[inline]
    fn step_tick(&mut self) -> Duration {
        self.inner.step_tick()
    }
}

// ============================================================================
// Worker Thread Main Event & Polling Loop
// ============================================================================

#[inline]
fn send_event(
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
    event: WorkerEvent,
) {
    if evt_tx.send(event).is_ok() {
        if let Some(ctx) = egui_ctx {
            ctx.request_repaint();
        }
    }
}

fn worker_thread_main(
    cmd_rx: Receiver<GuiCommand>,
    evt_tx: Sender<WorkerEvent>,
    ffb_abort_flag: Arc<AtomicBool>,
    egui_ctx: Option<egui::Context>,
    config: WorkerConfig,
) {
    let dinput_ctx = if config.enable_physical_dinput {
        match DirectInputContext::new() {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                send_event(
                    &evt_tx,
                    &egui_ctx,
                    WorkerEvent::Error(format!(
                        "Falha ao inicializar DirectInput8: {err}. O modo de simulação virtual permanece disponível."
                    )),
                );
                None
            }
        }
    } else {
        None
    };

    let mut cached_devices: Vec<DeviceInfo> = Vec::new();
    let mut active_dual: Option<(usize, DualDeviceHandle)> = None;

    // Initial USB enumeration on startup
    refresh_devices_list(
        dinput_ctx.as_ref(),
        &mut cached_devices,
        &mut active_dual,
        &evt_tx,
        &egui_ctx,
    );

    loop {
        let next_cmd = if active_dual.is_some() {
            match cmd_rx.recv_timeout(config.telemetry_poll_interval) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match cmd_rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };

        if let Some(cmd) = next_cmd {
            match cmd {
                GuiCommand::Shutdown => break,

                GuiCommand::RefreshDevices => {
                    refresh_devices_list(
                        dinput_ctx.as_ref(),
                        &mut cached_devices,
                        &mut active_dual,
                        &evt_tx,
                        &egui_ctx,
                    );
                }

                GuiCommand::SelectDevice(device_idx) => {
                    open_selected_dual_handle(
                        dinput_ctx.as_ref(),
                        &cached_devices,
                        device_idx,
                        &mut active_dual,
                        &evt_tx,
                        &egui_ctx,
                    );
                }

                GuiCommand::ApplyAxisCalibration {
                    device_idx,
                    axis_id,
                    min,
                    center,
                    max,
                    deadzone,
                    saturation,
                } => {
                    handle_apply_calibration(
                        dinput_ctx.as_ref(),
                        &mut cached_devices,
                        &mut active_dual,
                        device_idx,
                        axis_id,
                        min,
                        center,
                        max,
                        deadzone,
                        saturation,
                        &evt_tx,
                        &egui_ctx,
                    );
                }

                GuiCommand::RestoreDefaultCalibration {
                    device_idx,
                    axis_id,
                } => {
                    handle_restore_default_calibration(
                        dinput_ctx.as_ref(),
                        &mut cached_devices,
                        &mut active_dual,
                        device_idx,
                        axis_id,
                        &evt_tx,
                        &egui_ctx,
                    );
                }

                GuiCommand::StartStepLog2 {
                    device_idx,
                    steps,
                    duration_ms,
                    max_angle_deg,
                    max_force,
                    dry_run,
                } => {
                    let should_shutdown = handle_start_step_log2(
                        dinput_ctx.as_ref(),
                        &cached_devices,
                        &mut active_dual,
                        &cmd_rx,
                        &evt_tx,
                        &egui_ctx,
                        &ffb_abort_flag,
                        config,
                        device_idx,
                        steps,
                        duration_ms,
                        max_angle_deg,
                        max_force,
                        dry_run,
                    );
                    if should_shutdown {
                        break;
                    }
                }

                GuiCommand::EmergencyStopFfb => {
                    ffb_abort_flag.store(true, Ordering::SeqCst);
                    send_event(
                        &evt_tx,
                        &egui_ctx,
                        WorkerEvent::StatusMessage(
                            "Teste interrompido — motor desligado imediatamente.".to_string(),
                        ),
                    );
                }
            }
        }

        // Poll simultaneous Raw + Cooked handles at ~60 Hz when an active controller is selected
        if let Some((_, ref mut dual)) = active_dual {
            if let Ok(snapshots) = dual.poll_telemetry() {
                send_event(
                    &evt_tx,
                    &egui_ctx,
                    WorkerEvent::LiveAxisTelemetry(snapshots),
                );
            }
        }
    }
}

fn refresh_devices_list(
    dinput_ctx: Option<&DirectInputContext>,
    cached_devices: &mut Vec<DeviceInfo>,
    active_dual: &mut Option<(usize, DualDeviceHandle)>,
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
) {
    let Some(ctx) = dinput_ctx else {
        cached_devices.clear();
        *active_dual = None;
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::DeviceListUpdated(Vec::new()),
        );
        return;
    };

    // Release existing dual handle before re-enumerating USB bus cleanly
    let prev_idx = active_dual.as_ref().map(|(idx, _)| *idx);
    *active_dual = None;

    match ctx.enumerate_controllers() {
        Ok(devices) => {
            let count = devices.len();
            *cached_devices = devices.clone();
            send_event(evt_tx, egui_ctx, WorkerEvent::DeviceListUpdated(devices));

            if count == 0 {
                send_event(
                    evt_tx,
                    egui_ctx,
                    WorkerEvent::StatusMessage(
                        "Nenhum volante ou pedal USB conectado. Conecte seu equipamento e clique em Atualizar USB.".to_string(),
                    ),
                );
            } else {
                // Re-select previous index if still valid, else prefer first FFB wheel, else index 0
                let target_idx = prev_idx
                    .filter(|&i| i < count)
                    .or_else(|| cached_devices.iter().position(|d| d.has_ffb))
                    .unwrap_or(0);
                open_selected_dual_handle(
                    Some(ctx),
                    cached_devices,
                    target_idx,
                    active_dual,
                    evt_tx,
                    egui_ctx,
                );
            }
        }
        Err(err) => {
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(format!("Erro ao enumerar dispositivos USB DirectInput8: {err}")),
            );
        }
    }
}

fn open_selected_dual_handle(
    dinput_ctx: Option<&DirectInputContext>,
    cached_devices: &[DeviceInfo],
    device_idx: usize,
    active_dual: &mut Option<(usize, DualDeviceHandle)>,
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
) {
    let Some(ctx) = dinput_ctx else {
        return;
    };
    let Some(info) = cached_devices.get(device_idx).cloned() else {
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::Error(format!(
                "Controlador de índice #{device_idx} não encontrado (total conectados: {}).",
                cached_devices.len()
            )),
        );
        return;
    };

    // Drop previous handles before acquiring the newly selected controller
    *active_dual = None;

    match DualDeviceHandle::open(ctx, info.clone()) {
        Ok(mut dual) => {
            if let Ok(snapshots) = dual.poll_telemetry() {
                send_event(
                    evt_tx,
                    egui_ctx,
                    WorkerEvent::LiveAxisTelemetry(snapshots),
                );
            }
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::StatusMessage(format!(
                    "Controlador conectado: {} — {} eixo(s){}",
                    info.instance_name,
                    info.axes.len(),
                    if info.has_ffb {
                        " • Force Feedback ativo"
                    } else {
                        ""
                    }
                )),
            );
            *active_dual = Some((device_idx, dual));
        }
        Err(err) => {
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(format!(
                    "Não foi possível conectar ao controlador `{}`: {err}",
                    info.instance_name
                )),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_apply_calibration(
    dinput_ctx: Option<&DirectInputContext>,
    cached_devices: &mut [DeviceInfo],
    active_dual: &mut Option<(usize, DualDeviceHandle)>,
    device_idx: usize,
    axis_id: u32,
    min: i32,
    center: i32,
    max: i32,
    deadzone: Option<u32>,
    saturation: Option<u32>,
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
) {
    let needs_open = active_dual
        .as_ref()
        .map(|(idx, _)| *idx != device_idx)
        .unwrap_or(true);
    if needs_open {
        open_selected_dual_handle(
            dinput_ctx,
            cached_devices,
            device_idx,
            active_dual,
            evt_tx,
            egui_ctx,
        );
    }

    let Some((open_idx, ref mut dual)) = active_dual else {
        return;
    };

    match dual.apply_live_calibration(axis_id, min, center, max, deadzone, saturation) {
        Ok(outcome) => {
            if let Some(slot) = cached_devices.get_mut(*open_idx) {
                *slot = dual.info.clone();
            }
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::StatusMessage(format!(
                    "Ajuste salvo em `{}` com backup automático de segurança.",
                    outcome.axis_name
                )),
            );
            send_event(evt_tx, egui_ctx, WorkerEvent::CalibrationApplied(outcome));
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::DeviceListUpdated(cached_devices.to_vec()),
            );
        }
        Err(err) => {
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(format!("Não foi possível salvar a calibração do eixo: {err}")),
            );
        }
    }
}

fn handle_restore_default_calibration(
    dinput_ctx: Option<&DirectInputContext>,
    cached_devices: &mut [DeviceInfo],
    active_dual: &mut Option<(usize, DualDeviceHandle)>,
    device_idx: usize,
    axis_id: u32,
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
) {
    let Some(dev) = cached_devices.get(device_idx) else {
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::Error(format!(
                "Dispositivo #{device_idx} não disponível para restaurar calibração padrão."
            )),
        );
        return;
    };

    let target_axis = dev
        .axes
        .iter()
        .find(|a| {
            a.dw_type == axis_id
                || (a.axis_index as u32) == axis_id
                || (a.instance_id as u32) == axis_id
        })
        .cloned();

    let Some(axis) = target_axis else {
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::Error(format!(
                "Eixo selecionado não encontrado em `{}`.",
                dev.instance_name
            )),
        );
        return;
    };

    let (l_min, l_max) = axis.logical_range;
    let def_cal = AxisCalibration::default_reset(l_min, l_max);

    handle_apply_calibration(
        dinput_ctx,
        cached_devices,
        active_dual,
        device_idx,
        axis.dw_type,
        def_cal.min,
        def_cal.center,
        def_cal.max,
        Some(0),
        Some(10_000),
        evt_tx,
        egui_ctx,
    );
}

#[allow(clippy::too_many_arguments)]
fn handle_start_step_log2(
    dinput_ctx: Option<&DirectInputContext>,
    cached_devices: &[DeviceInfo],
    active_dual: &mut Option<(usize, DualDeviceHandle)>,
    cmd_rx: &Receiver<GuiCommand>,
    evt_tx: &Sender<WorkerEvent>,
    egui_ctx: &Option<egui::Context>,
    ffb_abort_flag: &Arc<AtomicBool>,
    worker_cfg: WorkerConfig,
    device_idx: usize,
    steps: u32,
    duration_ms: u64,
    max_angle_deg: f64,
    max_force: i32,
    dry_run: bool,
) -> bool {
    // Drain any queued EmergencyStopFfb / Shutdown received before this handler began
    while let Ok(queued) = cmd_rx.try_recv() {
        match queued {
            GuiCommand::Shutdown => return true,
            GuiCommand::EmergencyStopFfb => {
                ffb_abort_flag.store(true, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    if ffb_abort_flag.load(Ordering::SeqCst) {
        send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::StatusMessage(
                "Ensaio Step Log 2 cancelado antes do início (Parada de Emergência ativa)."
                    .to_string(),
            ),
        );
        return false;
    }

    let step_cfg = StepLog2Config {
        steps,
        duration_ms,
        settle_ms: DEFAULT_STEP_LOG2_SETTLE_MS,
        max_angle_deg,
        max_force,
        direction: 1,
    };

    if let Err(err) = step_cfg.validate() {
        send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::Error(format!("Parâmetros inválidos para Step Log 2: {err}")),
        );
        return false;
    }

    let resume_device_idx = active_dual.as_ref().map(|(idx, _)| *idx);

    let run_result: CoreResult<Vec<StepLogSample>> = if dry_run {
        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::StatusMessage(format!(
                "Executando leitura simulada de Force Feedback ({steps} passos)..."
            )),
        );
        let mut sim = VirtualWheelSimulator::new();
        let mut monitored =
            GuiMonitoredBackend::new(&mut sim, evt_tx, egui_ctx, steps, max_force);

        run_step_log2_with_backend(&mut monitored, &step_cfg, ffb_abort_flag, |prog| {
            let label = if prog.early_cutoff {
                "Limite de segurança atingido".to_string()
            } else if prog.step_index >= prog.total_steps {
                "Concluído".to_string()
            } else {
                "Centralizando volante".to_string()
            };
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::StepLog2Progress {
                    current_step: prog.step_index,
                    total_steps: prog.total_steps,
                    sample: prog.sample,
                    state_label: label,
                },
            );

            // Optional smooth visual pacing in `--dry-run` mode while polling abort every 1 ms
            if !worker_cfg.dry_run_step_delay.is_zero() {
                let slices = worker_cfg.dry_run_step_delay.as_millis() as u64;
                for _ in 0..slices.max(1) {
                    if ffb_abort_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            }
        })
    } else {
        let Some(ctx) = dinput_ctx else {
            send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(
                    "DirectInput8 indisponível neste sistema.".to_string(),
                ),
            );
            return false;
        };

        let Some(target_info) = cached_devices.get(device_idx).cloned() else {
            send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(
                    "Nenhum volante selecionado. Conecte o volante USB e tente novamente."
                        .to_string(),
                ),
            );
            return false;
        };

        // Release non-exclusive Raw/Cooked handles BEFORE acquiring DISCL_EXCLUSIVE for FFB
        *active_dual = None;

        send_event(
            evt_tx,
            egui_ctx,
            WorkerEvent::StatusMessage(format!(
                "Medindo resposta do volante `{}` — Pressione ESPAÇO ou ESC para parar a qualquer momento.",
                target_info.instance_name
            )),
        );

        let ffb_res = match FfbSession::open(ctx, target_info) {
            Ok(mut session) => {
                let mut monitored =
                    GuiMonitoredBackend::new(&mut session, evt_tx, egui_ctx, steps, max_force);
                run_step_log2_with_backend(&mut monitored, &step_cfg, ffb_abort_flag, |prog| {
                    let label = if prog.early_cutoff {
                        "Limite de segurança atingido".to_string()
                    } else if prog.step_index >= prog.total_steps {
                        "Concluído".to_string()
                    } else {
                        "Centralizando volante".to_string()
                    };
                    send_event(
                        evt_tx,
                        egui_ctx,
                        WorkerEvent::StepLog2Progress {
                            current_step: prog.step_index,
                            total_steps: prog.total_steps,
                            sample: prog.sample,
                            state_label: label,
                        },
                    );
                })
                // `session` (`FfbSession`) drops here unconditionally -> `Stop()` + `Unacquire()` + `timeEndPeriod(1)`
            }
            Err(err) => Err(err),
        };

        // Automatically reopen non-exclusive DualDeviceHandle so Tab 1 telemetry resumes seamlessly
        if let Some(prev_idx) = resume_device_idx {
            open_selected_dual_handle(
                Some(ctx),
                cached_devices,
                prev_idx,
                active_dual,
                evt_tx,
                egui_ctx,
            );
        }

        ffb_res
    };

    match run_result {
        Ok(samples) => {
            let count = samples.len().saturating_sub(1);
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::StatusMessage(format!(
                    "Teste concluído ({count} passos lidos). Curva LUT gerada automaticamente!"
                )),
            );
            send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Finished(samples));
        }
        Err(CoreError::UserAborted) => {
            send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::StatusMessage(
                    "Teste interrompido — motor desligado imediatamente e volante liberado."
                        .to_string(),
                ),
            );
        }
        Err(err) => {
            send_event(evt_tx, egui_ctx, WorkerEvent::StepLog2Aborted);
            send_event(
                evt_tx,
                egui_ctx,
                WorkerEvent::Error(format!("Erro durante o teste de Force Feedback: {err}")),
            );
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_worker_physical_dinput_refresh_devices() {
        // Single physical DirectInput8 enumeration smoke test through HardwareWorker
        let worker = HardwareWorker::spawn_with_config(
            None,
            WorkerConfig {
                telemetry_poll_interval: Duration::from_millis(50),
                dry_run_step_delay: Duration::ZERO,
                enable_physical_dinput: true,
            },
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received_device_list = false;
        while Instant::now() < deadline {
            if let Some(WorkerEvent::DeviceListUpdated(_devices)) = worker.try_recv() {
                received_device_list = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            received_device_list,
            "Physical DirectInput8 worker should emit initial DeviceListUpdated"
        );
    }

    #[test]
    fn test_worker_headless_dry_run_streams_progress_and_finishes() {
        let worker = HardwareWorker::spawn_with_config(
            None,
            WorkerConfig {
                telemetry_poll_interval: Duration::from_millis(20),
                dry_run_step_delay: Duration::ZERO,
                enable_physical_dinput: false,
            },
        );

        worker.start_step_log2(0, 12, 300, 900.0, 10_000, true);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut progress_steps = Vec::new();
        let mut state_changes = 0usize;
        let mut finished_samples: Option<Vec<StepLogSample>> = None;

        while Instant::now() < deadline {
            if let Some(evt) = worker.try_recv() {
                match evt {
                    WorkerEvent::StepLog2StateChanged { .. } => {
                        state_changes += 1;
                    }
                    WorkerEvent::StepLog2Progress {
                        current_step,
                        total_steps,
                        ..
                    } => {
                        assert_eq!(total_steps, 12);
                        progress_steps.push(current_step);
                    }
                    WorkerEvent::StepLog2Finished(samples) => {
                        finished_samples = Some(samples);
                        break;
                    }
                    WorkerEvent::Error(e) => panic!("Unexpected worker error in dry-run: {e}"),
                    _ => {}
                }
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }

        let samples = finished_samples.expect("Worker should emit StepLog2Finished");
        assert_eq!(progress_steps.len(), 12);
        assert_eq!(progress_steps[0], 1);
        assert_eq!(progress_steps[11], 12);
        assert!(state_changes >= 3, "Expected live intra-step state transitions");
        // Row 0 + 12 steps == 13 samples
        assert_eq!(samples.len(), 13);
        assert_eq!(samples[0], StepLogSample::ZERO);
        assert!(samples[12].delta_x_deg > 50.0);
    }

    #[test]
    fn test_worker_emergency_stop_aborts_in_flight_step_log2() {
        let worker = HardwareWorker::spawn_with_config(
            None,
            WorkerConfig {
                telemetry_poll_interval: Duration::from_millis(20),
                dry_run_step_delay: Duration::from_millis(15),
                enable_physical_dinput: false,
            },
        );

        worker.start_step_log2(0, 50, 300, 900.0, 10_000, true);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_first_progress = false;
        let mut aborted = false;

        while Instant::now() < deadline {
            if let Some(evt) = worker.try_recv() {
                match evt {
                    WorkerEvent::StepLog2Progress { current_step, .. } => {
                        if current_step >= 2 && !saw_first_progress {
                            saw_first_progress = true;
                            worker.trigger_emergency_stop();
                        }
                    }
                    WorkerEvent::StepLog2Aborted => {
                        aborted = true;
                        break;
                    }
                    WorkerEvent::StepLog2Finished(_) => {
                        panic!("StepLog2 should have aborted immediately on EmergencyStopFfb");
                    }
                    _ => {}
                }
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }

        assert!(saw_first_progress, "Should receive initial progress before abort");
        assert!(aborted, "Worker must emit StepLog2Aborted immediately after trigger_emergency_stop");
        assert!(worker.abort_flag().load(Ordering::SeqCst));
    }
}
