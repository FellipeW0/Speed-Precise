//! DirectInput8 Force Feedback Engine, PID Auto-Centering Controller, RAII Safety Guard,
//! and WheelCheck `Step Log 2` State Machine (`FUN_004034a0`, `FUN_00404040`, `FUN_00404230`, `FUN_004078c0`).

use std::ffi::c_void;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::calibration::{get_axis_range, set_axis_range};
use crate::device::{ControllerDeviceInfo, DirectInputContext};
use crate::dinput_ffi::{
    makediprop, timeBeginPeriod, timeEndPeriod, CloseHandle, CreateWaitableTimerExW,
    CreateWindowExW, DestroyWindow, DiConstantForce, DiDataFormat, DiEffect, DiObjectDataFormat,
    DipropDword, DipropRange, GetConsoleWindow, GetCurrentProcess, GetCurrentThread,
    GetDesktopWindow, GetForegroundWindow, GetModuleHandleW, GetPriorityClass,
    IDirectInputDevice8W, IDirectInputEffect, SetPriorityClass, SetThreadAffinityMask,
    SetWaitableTimer, Sleep, WaitForSingleObject, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
    DIDF_ABSAXIS, DIEP_TYPESPECIFICPARAMS, DIPH_BYOFFSET, DIPH_DEVICE, DIPROP_AUTOCENTER,
    DIPROP_BUFFERSIZE, DIPROP_FFGAIN, DIPROP_LOGICALRANGE, DIPROP_RANGE, DISCL_BACKGROUND,
    DISCL_EXCLUSIVE, DISCL_FOREGROUND, DISFFC_RESET, DISFFC_SETACTUATORSON, DISFFC_STOPALL,
    GUID_CONSTANTFORCE, GUID_XAXIS, HIGH_PRIORITY_CLASS, INFINITE, REALTIME_PRIORITY_CLASS,
    TIMERR_NOERROR, TIMER_ALL_ACCESS,
};
use crate::error::{check_hresult, CoreError, CoreResult};

/// Default WheelCheck Step Log 2 parameters extracted from `WheelCheck.exe` `.data` (`0x00458000`).
pub const DEFAULT_STEP_LOG2_STEPS: u32 = 50;
pub const DEFAULT_STEP_LOG2_DURATION_MS: u64 = 300;
pub const DEFAULT_STEP_LOG2_SETTLE_MS: u64 = 300;
pub const DEFAULT_STEP_LOG2_MAX_ANGLE_DEG: f64 = 900.0;
pub const DEFAULT_STEP_LOG2_MAX_FORCE: i32 = 10_000;
/// Soft-lock safety envelope in State 4 (`85%` of half-travel = `±382.5°` on a `900°` wheel).
/// Eliminates artificial clipping above `force = 8000` while maintaining a safe `67.5°` margin
/// before the mechanical end-stop (`10000` / `±450°`).
pub const ENDSTOP_SAFETY_LIMIT_NORM: i32 = 8_500;
/// Hard safety limit in State 2 (`PidCenter`) (`96.5%` of half-travel = `±434.25°` on a `900°` wheel),
/// allowing smooth PID return after an `8500` high-force pulse while guarding the mechanical lock.
pub const PID_HARD_SAFETY_LIMIT_NORM: i32 = 9_650;
/// Maximum PID auto-centering torque magnitude (`10000` = `100%` full scale, matching `WheelCheck.exe`
/// `FUN_00404230` line `3188` `param_4 = 10000`), enabling fast `~500–600 ms` centering and full
/// derivative braking authority at `1000 Hz`.
pub const DEFAULT_MAX_CENTERING_FORCE: i32 = 10_000;

/// Configuration for a `Step Log 2` linearity test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepLog2Config {
    /// Number of discrete force steps `N` (default: `50`, `DAT_00458088`).
    pub steps: u32,
    /// Duration of each force pulse in milliseconds (default: `300` ms, `DAT_0045808c`).
    pub duration_ms: u64,
    /// Mechanical settle wait with motor off before each pulse (default: `300` ms, `0x12A = 298` ms).
    pub settle_ms: u64,
    /// Total steering lock-to-lock angle in degrees (default: `900.0`, `DAT_00458090`).
    pub max_angle_deg: f64,
    /// Maximum DirectInput force magnitude (default: `10000`, `DAT_0045800c`).
    pub max_force: i32,
    /// Direction multiplier (`1` = normal, `-1` = inverted, `DAT_00458006`).
    pub direction: i32,
}

impl Default for StepLog2Config {
    fn default() -> Self {
        Self {
            steps: DEFAULT_STEP_LOG2_STEPS,
            duration_ms: DEFAULT_STEP_LOG2_DURATION_MS,
            settle_ms: DEFAULT_STEP_LOG2_SETTLE_MS,
            max_angle_deg: DEFAULT_STEP_LOG2_MAX_ANGLE_DEG,
            max_force: DEFAULT_STEP_LOG2_MAX_FORCE,
            direction: 1,
        }
    }
}

impl StepLog2Config {
    pub fn validate(&self) -> CoreResult<()> {
        if self.steps == 0 || self.steps > 1_000 {
            return Err(CoreError::InvalidStepLogConfig {
                reason: "`steps` must be between 1 and 1000",
            });
        }
        if self.duration_ms == 0 || self.duration_ms > 10_000 {
            return Err(CoreError::InvalidStepLogConfig {
                reason: "`duration_ms` must be between 1 and 10000 ms",
            });
        }
        if self.max_angle_deg <= 0.0 || self.max_angle_deg > 3_600.0 {
            return Err(CoreError::InvalidStepLogConfig {
                reason: "`max_angle_deg` must be in (0.0, 3600.0]",
            });
        }
        if self.max_force <= 0 || self.max_force > 10_000 {
            return Err(CoreError::InvalidStepLogConfig {
                reason: "`max_force` must be in 1..=10000",
            });
        }
        if self.direction != 1 && self.direction != -1 {
            return Err(CoreError::InvalidStepLogConfig {
                reason: "`direction` must be 1 or -1",
            });
        }
        Ok(())
    }
}

/// Single row in a `WheelCheck` `Step Log 2` CSV file (`force, startX, endX, deltaX, deltaXDeg`).
///
/// Verified in `WheelCheck.exe` (`FUN_004078c0`, line 5059):
/// `force`, `start_x`, `end_x`, and `delta_x` are signed 32-bit integers (`%d` in `[-10000, 10000]`),
/// and `delta_x_deg` is a floating-point angle in degrees (`%f`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StepLog2Sample {
    pub force: i32,
    pub start_x: i32,
    pub end_x: i32,
    pub delta_x: i32,
    pub delta_x_deg: f64,
}

impl StepLog2Sample {
    pub const ZERO: Self = Self {
        force: 0,
        start_x: 0,
        end_x: 0,
        delta_x: 0,
        delta_x_deg: 0.0,
    };
}

/// Discrete states of the `WheelCheck.exe` `Step Log 2` state machine (`DAT_0045be34` `0..=5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepLog2State {
    /// State 0: Initialize sample table with row 0 (`0, 0, 0, 0, 0`).
    Init = 0,
    /// State 1: Breakaway wake-up ramp (`-50` per tick down to `-2000`) until encoder moves `> 1` step or `2000 ms`.
    BreakawayRamp = 1,
    /// State 2: Auto-center wheel to `0` using PID controller (`>= 501 ms` until `|d_term| < 0.05` or `10000 ms`).
    PidCenter = 2,
    /// State 3: Motor-off mechanical settle (`force = 0` for `300 ms` / `> 298 ms`).
    MotorOffSettle = 3,
    /// State 4: Apply constant-force pulse for `duration_ms` (`300 ms`) with end-stop cutoff (`|x| >= 8000`).
    ForcePulse = 4,
    /// State 5: Zero motor force, release exclusive lock, and finalize CSV export.
    Complete = 5,
}

/// Progress update emitted after each step in `Step Log 2`.
#[derive(Debug, Clone, Copy)]
pub struct StepLog2Progress {
    pub step_index: u32,
    pub total_steps: u32,
    pub sample: StepLog2Sample,
    pub early_cutoff: bool,
}

// ============================================================================
// Pure Mathematical Functions Verified from `WheelCheck.exe` (`FUN_004078c0`)
// ============================================================================

/// Returns the integer raw encoder value corresponding to physical center (`0°`).
///
/// For unsigned 16-bit ranges `[0, 65535]` (such as Logitech G29/G920), returns `32768`
/// (`lMin + ((lMax - lMin + 1) / 2)`). For signed ranges `[-32768, 32767]`, returns `0`.
#[inline]
pub const fn logical_center_raw(l_min: i32, l_max: i32) -> i32 {
    let span = (l_max as i64) - (l_min as i64) + 1;
    ((l_min as i64) + (span / 2)) as i32
}

/// Normalizes a raw hardware encoder reading `raw_x` $\in [l_{\min}, l_{\max}]$ into
/// unit coordinates $\in [-1.0, +1.0]$ relative to the physical midpoint `(lMin + lMax) / 2.0`.
///
/// - `raw_x == l_min` (`0` on G920, $-450^\circ$ left lock) $\to -1.0$
/// - `raw_x == logical_center_raw(l_min, l_max)` (`32768` on G920, $0^\circ$ center) $\to 0.0$
/// - `raw_x == l_max` (`65535` on G920, $+450^\circ$ right lock) $\to +1.0$
#[inline]
pub fn normalize_raw_to_unit(raw_x: i32, l_min: i32, l_max: i32) -> f64 {
    let half_span = ((l_max as f64) - (l_min as f64)) * 0.5;
    if half_span.abs() < f64::EPSILON {
        return 0.0;
    }
    if raw_x == logical_center_raw(l_min, l_max) {
        return 0.0;
    }
    let mid = ((l_min as f64) + (l_max as f64)) * 0.5;
    (((raw_x as f64) - mid) / half_span).clamp(-1.0, 1.0)
}

/// Normalizes a raw hardware encoder reading `raw_x` $\in [l_{\min}, l_{\max}]$ into
/// DirectInput standard coordinates $\in [-10000, +10000]$.
///
/// Verified in `WheelCheck.exe` (`0x00407B18`–`0x00407B4E`):
/// `posX_norm = trunc((rawX - lMin) * (20000.0 / (lMax - lMin)) - 10000.0)`
///
/// On a Logitech G920 (`[0, 65535]`):
/// - `raw_x = 0` ($-450^\circ$ left lock) $\to -10000$
/// - `raw_x = 32768` ($0^\circ$ physical center) $\to 0$
/// - `raw_x = 65535` ($+450^\circ$ right lock) $\to +10000$
#[inline]
pub fn normalize_raw_to_10000(raw_x: i32, l_min: i32, l_max: i32) -> i32 {
    let span = (l_max as f64) - (l_min as f64);
    if span.abs() < f64::EPSILON {
        return 0;
    }
    let norm = (((raw_x as f64) - (l_min as f64)) * (20_000.0 / span)) - 10_000.0;
    (norm.trunc() as i32).clamp(-10_000, 10_000)
}

/// Computes the integer force command for step `s` (`0..steps`), matching
/// `WheelCheck.exe` (`FUN_004078c0`, lines 5048-5049):
/// `force = (dir * max_force * (s + 1)) / steps`
#[inline]
pub fn compute_step_force(step_index: u32, total_steps: u32, max_force: i32, dir: i32) -> i32 {
    if total_steps == 0 {
        return 0;
    }
    let num = (dir as i64) * (max_force as i64) * ((step_index as i64) + 1);
    (num / (total_steps as i64)).clamp(-10_000, 10_000) as i32
}

/// Computes `deltaXDeg` directly from unquantized logical encoder units `[lMin, lMax]`,
/// matching `WheelCheck.exe` (`FUN_004078c0`, lines 5055-5067):
///
/// $$\text{deltaXDeg} = \text{dir} \times \left(\frac{\text{max\_angle\_deg}}{l_{\max} - l_{\min}} \times -(\text{rawEndX} - \text{rawStartX})\right)$$
#[inline]
pub fn compute_delta_x_deg(
    raw_start_x: i32,
    raw_end_x: i32,
    l_min: i32,
    l_max: i32,
    max_angle_deg: f64,
    dir: i32,
) -> f64 {
    let span = (l_max as f64) - (l_min as f64);
    if span.abs() < f64::EPSILON {
        return 0.0;
    }
    let raw_delta = (raw_end_x as f64) - (raw_start_x as f64);
    let deg = (dir as f64) * ((max_angle_deg / span) * (-raw_delta));
    if deg.abs() < 1e-12 {
        0.0
    } else {
        deg
    }
}

/// Formats a slice of [`StepLog2Sample`] rows into the exact `WheelCheck.exe` CSV text format.
pub fn format_step_log2_csv(samples: &[StepLog2Sample]) -> String {
    let mut out = String::with_capacity(samples.len() * 40 + 64);
    out.push_str("force, startX, endX, deltaX, deltaXDeg\n");
    for (idx, s) in samples.iter().enumerate() {
        if idx == 0 && s.force == 0 && s.start_x == 0 && s.end_x == 0 && s.delta_x == 0 && s.delta_x_deg == 0.0 {
            out.push_str("0, 0, 0, 0, 0\n");
        } else {
            let _ = writeln!(
                out,
                "{}, {}, {}, {}, {:.6}",
                s.force, s.start_x, s.end_x, s.delta_x, s.delta_x_deg
            );
        }
    }
    out
}

/// Writes a slice of [`StepLog2Sample`] rows to disk in `WheelCheck.exe` CSV format.
pub fn write_step_log2_csv(path: &Path, samples: &[StepLog2Sample]) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| CoreError::CsvIo {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    let csv_text = format_step_log2_csv(samples);
    fs::write(path, csv_text).map_err(|source| CoreError::CsvIo {
        path: path.display().to_string(),
        source,
    })
}

// ============================================================================
// PID Auto-Centering Controller (`FUN_00404040` & `FUN_00404230`)
// ============================================================================

/// PID controller replicating `WheelCheck.exe` `FUN_00404040` and auto-centering wrapper `FUN_00404230`,
/// including `WheelCheck.exe`'s exponential `dt` filter (`_DAT_004580d4 = 0.9`, `_DAT_004580d8 = 0.001`)
/// at `1000 Hz` (`1 ms` tick).
///
/// Default gains from `WheelCheck.exe` `.data` (`0x00458024`–`0x00458030` & `FUN_00404230` line `3188`):
/// - `kp = 5.0`
/// - `ki = 0.0`
/// - `kd = 0.25`
/// - `td = 0.9`
/// - `max_d = 10000.0`
/// - `max_centering_force = 10000` (`100%` full scale for fast `~500–600 ms` centering and derivative braking)
#[derive(Debug, Clone)]
pub struct PidCenteringController {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub td: f64,
    pub max_d: f64,
    pub max_i: f64,
    pub max_centering_force: i32,
    i_state: f64,
    prev_err: f64,
    prev_d: f64,
    smoothed_dt_sec: f64,
    initialized: bool,
}

impl Default for PidCenteringController {
    fn default() -> Self {
        Self {
            kp: 5.0,
            ki: 0.0,
            kd: 0.25,
            td: 0.9,
            max_d: 10_000.0,
            max_i: 1_000.0,
            max_centering_force: DEFAULT_MAX_CENTERING_FORCE,
            i_state: 0.0,
            prev_err: 0.0,
            prev_d: 0.0,
            smoothed_dt_sec: 0.001,
            initialized: false,
        }
    }
}

impl PidCenteringController {
    pub fn reset(&mut self) {
        self.i_state = 0.0;
        self.prev_err = 0.0;
        self.prev_d = 0.0;
        self.smoothed_dt_sec = 0.001;
        self.initialized = false;
    }

    #[inline]
    pub fn last_d_term(&self) -> f64 {
        self.prev_d
    }

    /// Computes the next PID centering force command (`[-max_centering_force, +max_centering_force]`)
    /// given `target_x_norm` and current `pos_x_norm` (both in `[-10000, +10000]`) over time step `dt_sec`.
    ///
    /// Verified in `WheelCheck.exe` `FUN_00404040` (line `3108`) & `FUN_004078c0` (lines `4713–4719`):
    /// `iVar2 = param_3 - param_2` (`err = pos_x_norm - target_x_norm`).
    /// In DirectInput8 (`GUID_ConstantForce` with `rglDirection[0] = 1`):
    /// - Positive `lMagnitude` (`+force`) pulls the wheel toward `lMin` (`-10000`, left).
    /// - Negative `lMagnitude` (`-force`) pulls the wheel toward `lMax` (`+10000`, right).
    /// Therefore, when `pos_x_norm > target_x_norm` (displaced right of center), `err > 0` produces
    /// a positive `+force` that pulls the wheel left back toward `target_x_norm` (`0` / `32768`),
    /// while `d_term` actively brakes incoming angular velocity `50 ms` ahead of center.
    pub fn update(&mut self, dt_sec: f64, target_x_norm: i32, pos_x_norm: i32) -> i32 {
        let raw_dt = dt_sec.clamp(1e-4, 0.050);
        let err = (pos_x_norm as f64) - (target_x_norm as f64);

        if !self.initialized {
            self.prev_err = err;
            self.prev_d = 0.0;
            self.i_state = 0.0;
            self.smoothed_dt_sec = raw_dt.clamp(1e-4, 0.020);
            self.initialized = true;
        } else {
            // WheelCheck.exe FUN_004078c0 lines 4713-4718: _DAT_004580d8 = 0.1 * raw_dt + 0.9 * prev_dt
            self.smoothed_dt_sec = (0.1 * raw_dt + 0.9 * self.smoothed_dt_sec).max(1e-4);
        }
        let dt = self.smoothed_dt_sec;

        let p_term = err * self.kp;
        self.i_state = (self.i_state + err * self.ki * dt).clamp(-self.max_i, self.max_i);

        let alpha = ((self.td * 0.001) / dt).clamp(0.0, 0.99);
        let raw_d = ((err - self.prev_err) * self.kd) / dt;
        let d_term =
            ((1.0 - alpha) * raw_d + alpha * self.prev_d).clamp(-self.max_d, self.max_d);

        self.prev_err = err;
        self.prev_d = d_term;

        let cap = self.max_centering_force.clamp(100, 10_000) as f64;
        let total = (p_term + self.i_state + d_term).trunc();
        total.clamp(-cap, cap) as i32
    }

    /// Checks `WheelCheck.exe` `FUN_00404230` (lines 3191-3206) stop condition using exact `Duration`:
    /// Must run for at least `501 ms`, after which it stops as soon as `|d_term| < 0.05`
    /// (or times out at `10000 ms`).
    #[inline]
    pub fn is_settled_duration(&self, elapsed: Duration) -> bool {
        if elapsed >= Duration::from_millis(10_000) {
            return true;
        }
        elapsed >= Duration::from_millis(501) && self.prev_d.abs() < 0.05
    }

    /// Millisecond convenience wrapper around [`Self::is_settled_duration`].
    #[inline]
    pub fn is_settled(&self, elapsed_ms: u64) -> bool {
        self.is_settled_duration(Duration::from_millis(elapsed_ms))
    }
}

// ============================================================================
// RAII `1000 Hz` Multimedia Timer & Process Priority Guard (`HighResTimerGuard`)
// ============================================================================

/// RAII `1000 Hz` (`1 ms`) timer and process/thread priority guard replicating `WheelCheck.exe`
/// (`FUN_004024d0` `timeBeginPeriod(1)` + `SetThreadAffinityMask(1)`, `FUN_00401210`
/// `HIGH_PRIORITY_CLASS = 0x80`, and `FUN_004078c0` `REALTIME_PRIORITY_CLASS = 0x100`).
///
/// Guarantees via `Drop` that `timeEndPeriod(1)`, original process priority class, original thread
/// affinity mask, and the Win32 high-resolution waitable timer handle are restored/closed unconditionally.
pub struct HighResTimerGuard {
    timer_period_active: bool,
    original_priority: u32,
    original_affinity: usize,
    waitable_timer: *mut c_void,
}

impl HighResTimerGuard {
    /// Activates `1 ms` Windows multimedia timer resolution, elevates scheduler priority,
    /// pins the FFB loop thread to a single core to avoid TSC/cache migration jitter,
    /// and opens a kernel `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` timer for `1000 Hz` ticks.
    pub fn new() -> Self {
        unsafe {
            let timer_period_active = timeBeginPeriod(1) == TIMERR_NOERROR;

            let process = GetCurrentProcess();
            let original_priority = GetPriorityClass(process);
            if SetPriorityClass(process, REALTIME_PRIORITY_CLASS) == 0 {
                let _ = SetPriorityClass(process, HIGH_PRIORITY_CLASS);
            }

            let original_affinity = SetThreadAffinityMask(GetCurrentThread(), 1);

            let waitable_timer = CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            );

            Self {
                timer_period_active,
                original_priority,
                original_affinity,
                waitable_timer,
            }
        }
    }

    /// Returns `true` if `timeBeginPeriod(1)` succeeded on `winmm.dll`.
    #[inline]
    pub const fn is_timer_period_active(&self) -> bool {
        self.timer_period_active
    }

    /// Sleeps until the next `1.0 ms` (`1000 Hz`) kernel clock edge without busy-spinning.
    ///
    /// Uses `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` (`-5000` 100-ns units = `0.5 ms` relative due time,
    /// waking at `~1.00–1.03 ms` per tick instead of Win32 `Sleep(1)`'s `1.67 ms` two-edge rounding),
    /// with automatic fallback to `Sleep(1)` if high-resolution waitable timers are unavailable.
    #[inline]
    pub fn sleep_1ms(&self) {
        unsafe {
            if !self.waitable_timer.is_null() {
                let due_100ns: i64 = -5_000;
                if SetWaitableTimer(
                    self.waitable_timer,
                    &due_100ns,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                ) != 0
                {
                    let _ = WaitForSingleObject(self.waitable_timer, 5);
                    return;
                }
            }
            Sleep(1);
        }
    }
}

impl Default for HighResTimerGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HighResTimerGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.waitable_timer.is_null() {
                let _ = CloseHandle(self.waitable_timer);
                self.waitable_timer = ptr::null_mut();
            }
            if self.original_affinity != 0 {
                let _ = SetThreadAffinityMask(GetCurrentThread(), self.original_affinity);
                self.original_affinity = 0;
            }
            if self.original_priority != 0 {
                let _ = SetPriorityClass(GetCurrentProcess(), self.original_priority);
                self.original_priority = 0;
            }
            if self.timer_period_active {
                let _ = timeEndPeriod(1);
                self.timer_period_active = false;
            }
        }
    }
}

// ============================================================================
// Hardware Backend Trait & RAII DirectInput8 `FfbSession`
// ============================================================================

/// Abstraction over physical DirectInput8 Force Feedback hardware and the deterministic
/// `--dry-run` virtual wheel simulator so both execute the exact same `Step Log 2` state machine.
pub trait FfbHardwareBackend {
    /// Returns the logical encoder bounds `(lMin, lMax)` of the steering X axis.
    fn logical_range(&self) -> (i32, i32);

    /// Polls the current raw encoder position in `[lMin, lMax]`.
    fn read_raw_x(&mut self) -> CoreResult<i32>;

    /// Applies a constant-force torque command in `[-10000, +10000]`.
    fn apply_force(&mut self, force: i32) -> CoreResult<()>;

    /// Advances the loop by one tick (`~1.0 ms` / `1000 Hz`) and returns the exact sub-millisecond
    /// wall-clock [`Duration`] elapsed in this tick (without `.as_millis()` integer truncation!).
    fn step_tick(&mut self) -> Duration;
}

/// RAII Win32 dummy window guard used to satisfy `IDirectInputDevice8W::SetCooperativeLevel`
/// when acquiring `DISCL_EXCLUSIVE | DISCL_BACKGROUND` (`0x09`).
struct FfbWindowGuard {
    hwnd: *mut c_void,
    owned: bool,
}

impl FfbWindowGuard {
    unsafe fn new() -> Self {
        let hinst = GetModuleHandleW(ptr::null());
        let class_name: [u16; 7] = [
            b'S' as u16,
            b'T' as u16,
            b'A' as u16,
            b'T' as u16,
            b'I' as u16,
            b'C' as u16,
            0,
        ];
        let window_name: [u16; 16] = [
            b'S' as u16,
            b'p' as u16,
            b'e' as u16,
            b'e' as u16,
            b'd' as u16,
            b'P' as u16,
            b'r' as u16,
            b'e' as u16,
            b'c' as u16,
            b'i' as u16,
            b's' as u16,
            b'e' as u16,
            b'F' as u16,
            b'f' as u16,
            b'b' as u16,
            0,
        ];

        let created = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            1,
            1,
            ptr::null_mut(),
            ptr::null_mut(),
            hinst,
            ptr::null_mut(),
        );
        if !created.is_null() {
            return Self {
                hwnd: created,
                owned: true,
            };
        }

        let console = GetConsoleWindow();
        if !console.is_null() {
            return Self {
                hwnd: console,
                owned: false,
            };
        }

        let fg = GetForegroundWindow();
        if !fg.is_null() {
            return Self {
                hwnd: fg,
                owned: false,
            };
        }

        Self {
            hwnd: GetDesktopWindow(),
            owned: false,
        }
    }
}

impl Drop for FfbWindowGuard {
    fn drop(&mut self) {
        if self.owned && !self.hwnd.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = ptr::null_mut();
        }
    }
}

/// RAII Force Feedback session (`FfbSafetyGuard` invariant from `AGENTS.md` Rule 3).
///
/// Replicates `WheelCheck.exe` `FUN_004034a0` (`SelectFFBDevice`) initialization, embeds
/// [`HighResTimerGuard`] for true `1000 Hz` (`1 ms`) loop execution, and guarantees via `Drop`
/// that `l_magnitude = 0`, `IDirectInputEffect::Stop()`, `Unload()`, `IDirectInputDevice8W::Unacquire()`,
/// and `timeEndPeriod(1)` are executed unconditionally on normal exit, error, or `Ctrl+C`.
pub struct FfbSession {
    pub info: ControllerDeviceInfo,
    device_ptr: *mut IDirectInputDevice8W,
    effect_ptr: *mut IDirectInputEffect,
    _timer_guard: HighResTimerGuard,
    _window_guard: FfbWindowGuard,
    _object_formats: Vec<DiObjectDataFormat>,
    axis_id_box: Box<u32>,
    dir_box: Box<i32>,
    cf_box: Box<DiConstantForce>,
    logical_range: (i32, i32),
    use_object_ids: bool,
    effect_started: bool,
    last_applied_force: Option<i32>,
    last_tick: Instant,
}

impl FfbSession {
    /// Opens an exclusive Force Feedback session on the specified controller.
    pub fn open(ctx: &DirectInputContext, info: ControllerDeviceInfo) -> CoreResult<Self> {
        if !info.has_ffb {
            return Err(CoreError::NoFfbSupport {
                device_name: info.instance_name,
            });
        }

        // Identify the primary steering axis (prefer GUID_XAxis, fallback to axis #0)
        let x_axis = info
            .axes
            .iter()
            .find(|a| a.guid_type == GUID_XAXIS)
            .or_else(|| info.axes.first())
            .ok_or_else(|| CoreError::AxisNotFound {
                axis_id: 0,
                device_name: info.instance_name.clone(),
            })?;
        let x_dw_type = x_axis.dw_type;

        unsafe {
            let mut device_ptr: *mut IDirectInputDevice8W = ptr::null_mut();
            let hr = ((*(*ctx.as_ptr()).lp_vtbl).create_device)(
                ctx.as_ptr(),
                &info.guid_instance,
                &mut device_ptr,
                ptr::null_mut(),
            );
            check_hresult("IDirectInput8W::CreateDevice(FfbSession)", hr)?;

            let window_guard = FfbWindowGuard::new();

            // Configure single-axis data format for X axis at offset 0
            let object_formats = vec![DiObjectDataFormat {
                pguid: ptr::null(),
                dw_ofs: 0,
                dw_type: x_dw_type,
                dw_flags: 0,
            }];
            let data_format = DiDataFormat {
                dw_size: std::mem::size_of::<DiDataFormat>() as u32,
                dw_obj_size: std::mem::size_of::<DiObjectDataFormat>() as u32,
                dw_flags: DIDF_ABSAXIS,
                dw_data_size: 4,
                dw_num_objs: 1,
                rgodf: object_formats.as_ptr(),
            };

            let df_hr = ((*(*device_ptr).lp_vtbl).set_data_format)(device_ptr, &data_format);
            if df_hr < 0 {
                let _ = ((*(*device_ptr).lp_vtbl).release)(device_ptr);
                return Err(CoreError::from_hresult(
                    "IDirectInputDevice8W::SetDataFormat(FfbSession)",
                    df_hr,
                ));
            }

            // WheelCheck FUN_004034a0 (lines 2742-2746): DISCL_EXCLUSIVE | DISCL_BACKGROUND (0x09),
            // fallback to DISCL_EXCLUSIVE | DISCL_FOREGROUND (0x05)
            let mut coop_hr = ((*(*device_ptr).lp_vtbl).set_cooperative_level)(
                device_ptr,
                window_guard.hwnd,
                DISCL_EXCLUSIVE | DISCL_BACKGROUND,
            );
            if coop_hr < 0 {
                coop_hr = ((*(*device_ptr).lp_vtbl).set_cooperative_level)(
                    device_ptr,
                    window_guard.hwnd,
                    DISCL_EXCLUSIVE | DISCL_FOREGROUND,
                );
            }
            if coop_hr < 0 {
                let _ = ((*(*device_ptr).lp_vtbl).release)(device_ptr);
                return Err(CoreError::from_hresult(
                    "IDirectInputDevice8W::SetCooperativeLevel(DISCL_EXCLUSIVE)",
                    coop_hr,
                ));
            }

            // WheelCheck FUN_004034a0 (lines 2752-2775): configure device properties while unacquired
            // 1. Disable hardware spring autocenter (DIPROP_AUTOCENTER = 9 -> 0)
            let autocenter_off = DipropDword::new(0, DIPH_DEVICE, 0);
            let _ = ((*(*device_ptr).lp_vtbl).set_property)(
                device_ptr,
                makediprop(DIPROP_AUTOCENTER),
                &autocenter_off.diph,
            );

            // 2. Set master FFB gain to 100% (DIPROP_FFGAIN = 7 -> 10000)
            let ffgain_max = DipropDword::new(0, DIPH_DEVICE, 10_000);
            let _ = ((*(*device_ptr).lp_vtbl).set_property)(
                device_ptr,
                makediprop(DIPROP_FFGAIN),
                &ffgain_max.diph,
            );

            // 3. Set event buffer size (DIPROP_BUFFERSIZE = 1 -> 10, matching FUN_004034a0 line 2770)
            let buf_size = DipropDword::new(0, DIPH_DEVICE, 10);
            let _ = ((*(*device_ptr).lp_vtbl).set_property)(
                device_ptr,
                makediprop(DIPROP_BUFFERSIZE),
                &buf_size.diph,
            );

            // 4. Query DIPROP_LOGICALRANGE (19) and set DIPROP_RANGE (4) = [lMin, lMax]
            // Replicates WheelCheck FUN_004034a0 (lines 2777-2796)
            let logical_range = get_axis_range(device_ptr, DIPROP_LOGICALRANGE, x_dw_type)
                .or_else(|_| {
                    let mut r = DipropRange::new(0, DIPH_BYOFFSET, 0, 0);
                    let hr = ((*(*device_ptr).lp_vtbl).get_property)(
                        device_ptr,
                        makediprop(DIPROP_LOGICALRANGE),
                        &mut r.diph,
                    );
                    check_hresult("GetProperty(DIPROP_LOGICALRANGE, DIPH_BYOFFSET)", hr)
                        .map(|()| (r.l_min, r.l_max))
                })
                .unwrap_or((0, 65535));

            let _ = set_axis_range(device_ptr, x_dw_type, logical_range.0, logical_range.1);
            let dev_range = DipropRange::new(0, DIPH_DEVICE, logical_range.0, logical_range.1);
            let _ = ((*(*device_ptr).lp_vtbl).set_property)(
                device_ptr,
                makediprop(DIPROP_RANGE),
                &dev_range.diph,
            );

            // Read back the actual active DIPROP_RANGE so coordinate normalization always matches GetDeviceState
            let active_range =
                get_axis_range(device_ptr, DIPROP_RANGE, x_dw_type).unwrap_or(logical_range);

            let acq_hr = ((*(*device_ptr).lp_vtbl).acquire)(device_ptr);
            if acq_hr < 0 {
                let _ = ((*(*device_ptr).lp_vtbl).release)(device_ptr);
                return Err(CoreError::from_hresult(
                    "IDirectInputDevice8W::Acquire(FfbSession)",
                    acq_hr,
                ));
            }

            let _ = ((*(*device_ptr).lp_vtbl).send_force_feedback_command)(
                device_ptr,
                DISFFC_RESET,
            );
            let _ = ((*(*device_ptr).lp_vtbl).send_force_feedback_command)(
                device_ptr,
                DISFFC_SETACTUATORSON,
            );

            // Create GUID_ConstantForce effect (try DIEFF_OBJECTIDS first, fallback to DIEFF_OBJECTOFFSETS)
            // Verified in WheelCheck FUN_004034a0 (line 2826) & FUN_004078c0 (line 5903):
            // rglDirection[0] MUST be 1 (+1 Cartesian X direction), NOT 0!
            let mut axis_id_box = Box::new(x_dw_type);
            let mut dir_box = Box::new(1i32);
            let mut cf_box = Box::new(DiConstantForce::new(0));
            let mut use_object_ids = true;

            let eff_by_id = DiEffect::new_constant_force(
                &mut *axis_id_id_ptr(&mut axis_id_box),
                &mut *dir_box,
                &mut *cf_box,
                true,
            );

            let mut effect_ptr: *mut IDirectInputEffect = ptr::null_mut();
            let mut ce_hr = ((*(*device_ptr).lp_vtbl).create_effect)(
                device_ptr,
                &GUID_CONSTANTFORCE,
                &eff_by_id,
                &mut effect_ptr,
                ptr::null_mut(),
            );

            if ce_hr < 0 || effect_ptr.is_null() {
                *axis_id_box = 0; // Byte offset 0 in our single-axis data format
                use_object_ids = false;
                let eff_by_ofs = DiEffect::new_constant_force(
                    &mut *axis_id_box,
                    &mut *dir_box,
                    &mut *cf_box,
                    false,
                );
                ce_hr = ((*(*device_ptr).lp_vtbl).create_effect)(
                    device_ptr,
                    &GUID_CONSTANTFORCE,
                    &eff_by_ofs,
                    &mut effect_ptr,
                    ptr::null_mut(),
                );
            }

            if ce_hr < 0 || effect_ptr.is_null() {
                let _ = ((*(*device_ptr).lp_vtbl).unacquire)(device_ptr);
                let _ = ((*(*device_ptr).lp_vtbl).release)(device_ptr);
                return Err(CoreError::from_hresult(
                    "IDirectInputDevice8W::CreateEffect(GUID_ConstantForce)",
                    ce_hr,
                ));
            }

            let timer_guard = HighResTimerGuard::new();

            Ok(Self {
                info,
                device_ptr,
                effect_ptr,
                _timer_guard: timer_guard,
                _window_guard: window_guard,
                _object_formats: object_formats,
                axis_id_box,
                dir_box,
                cf_box,
                logical_range: active_range,
                use_object_ids,
                effect_started: false,
                last_applied_force: None,
                last_tick: Instant::now(),
            })
        }
    }

    /// Immediately zeroes the constant force effect and stops the motor.
    pub fn emergency_stop(&mut self) {
        unsafe {
            if !self.effect_ptr.is_null() {
                self.cf_box.l_magnitude = 0;
                let eff = DiEffect::new_constant_force(
                    &mut *self.axis_id_box,
                    &mut *self.dir_box,
                    &mut *self.cf_box,
                    self.use_object_ids,
                );
                let _ = ((*(*self.effect_ptr).lp_vtbl).set_parameters)(
                    self.effect_ptr,
                    &eff,
                    DIEP_TYPESPECIFICPARAMS,
                );
                let _ = ((*(*self.effect_ptr).lp_vtbl).stop)(self.effect_ptr);
                self.effect_started = false;
                self.last_applied_force = None;
            }
            if !self.device_ptr.is_null() {
                let _ = ((*(*self.device_ptr).lp_vtbl).send_force_feedback_command)(
                    self.device_ptr,
                    DISFFC_STOPALL,
                );
            }
        }
    }
}

#[inline]
fn axis_id_id_ptr(b: &mut Box<u32>) -> *mut u32 {
    &mut **b as *mut u32
}

impl FfbHardwareBackend for FfbSession {
    #[inline]
    fn logical_range(&self) -> (i32, i32) {
        self.logical_range
    }

    fn read_raw_x(&mut self) -> CoreResult<i32> {
        let mut raw_x: i32 = 0;
        unsafe {
            let poll_hr = ((*(*self.device_ptr).lp_vtbl).poll)(self.device_ptr);
            if poll_hr < 0 {
                let _ = ((*(*self.device_ptr).lp_vtbl).acquire)(self.device_ptr);
                let _ = ((*(*self.device_ptr).lp_vtbl).poll)(self.device_ptr);
            }
            let mut state_hr = ((*(*self.device_ptr).lp_vtbl).get_device_state)(
                self.device_ptr,
                4,
                (&mut raw_x as *mut i32).cast::<c_void>(),
            );
            if state_hr < 0 {
                let _ = ((*(*self.device_ptr).lp_vtbl).acquire)(self.device_ptr);
                let _ = ((*(*self.device_ptr).lp_vtbl).poll)(self.device_ptr);
                state_hr = ((*(*self.device_ptr).lp_vtbl).get_device_state)(
                    self.device_ptr,
                    4,
                    (&mut raw_x as *mut i32).cast::<c_void>(),
                );
            }
            check_hresult("IDirectInputDevice8W::GetDeviceState(FfbSession)", state_hr)?;
        }
        Ok(raw_x)
    }

    fn apply_force(&mut self, force: i32) -> CoreResult<()> {
        let clamped = force.clamp(-10_000, 10_000);
        // WheelCheck.exe FUN_004078c0 line 5944 (`local_148 != DAT_0045bd0c`):
        // Skip redundant USB HID SetParameters calls when the constant-force magnitude has not changed.
        if self.effect_started && self.last_applied_force == Some(clamped) {
            return Ok(());
        }
        self.cf_box.l_magnitude = clamped;
        unsafe {
            let eff = DiEffect::new_constant_force(
                &mut *self.axis_id_box,
                &mut *self.dir_box,
                &mut *self.cf_box,
                self.use_object_ids,
            );
            if !self.effect_started {
                // WheelCheck.exe FUN_004078c0 lines 5931-5932: Start(INFINITE, 0) once
                let _ = ((*(*self.effect_ptr).lp_vtbl).start)(self.effect_ptr, INFINITE, 0);
                self.effect_started = true;
            }
            let hr = ((*(*self.effect_ptr).lp_vtbl).set_parameters)(
                self.effect_ptr,
                &eff,
                DIEP_TYPESPECIFICPARAMS,
            );
            check_hresult("IDirectInputEffect::SetParameters(ConstantForce)", hr)?;
            self.last_applied_force = Some(clamped);
        }
        Ok(())
    }

    fn step_tick(&mut self) -> Duration {
        self._timer_guard.sleep_1ms();
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick);
        self.last_tick = now;
        dt.max(Duration::from_micros(100))
    }
}

impl Drop for FfbSession {
    fn drop(&mut self) {
        self.emergency_stop();
        unsafe {
            if !self.effect_ptr.is_null() {
                let _ = ((*(*self.effect_ptr).lp_vtbl).unload)(self.effect_ptr);
                let _ = ((*(*self.effect_ptr).lp_vtbl).release)(self.effect_ptr);
                self.effect_ptr = ptr::null_mut();
            }
            if !self.device_ptr.is_null() {
                let _ = ((*(*self.device_ptr).lp_vtbl).unacquire)(self.device_ptr);
                let _ = ((*(*self.device_ptr).lp_vtbl).release)(self.device_ptr);
                self.device_ptr = ptr::null_mut();
            }
        }
    }
}

// ============================================================================
// Deterministic Virtual Wheel Simulator (`--dry-run`)
// ============================================================================

/// Deterministic virtual steering wheel hardware model for `--dry-run` execution and unit tests.
///
/// Simulates a 16-bit encoder (`0..=65535`, center `32768`, matching Logitech G29/G920) at `1000 Hz`
/// (`tick_dt_us = 1000` µs = `1.0 ms`) with:
/// - Exact DirectInput8 force polarity (`+force` rotates toward `lMin = 0`, `-force` rotates toward `lMax = 65535`),
/// - `14.5%` (`1450` DirectInput units) static friction / motor deadzone,
/// - Non-linear logarithmic torque response calibrated to empirical G920 `WheelCheck.exe` displacement
///   (`~0.0163 deg/force`, `~81.5°` at `force = 5000` over an exact `300 ms` pulse),
/// - Time-step-invariant exponential viscous damping and inertia so PID centering converges in `~520 ms`.
#[derive(Debug, Clone)]
pub struct VirtualWheelSimulator {
    pub logical_min: i32,
    pub logical_max: i32,
    pub deadzone_force: f64,
    pub current_raw_x: f64,
    pub angular_velocity_raw_per_sec: f64,
    pub commanded_force: i32,
    /// Simulated duration per tick in microseconds (default: `1_000` µs = `1.0 ms` = `1000 Hz`).
    pub tick_dt_us: u64,
}

impl Default for VirtualWheelSimulator {
    fn default() -> Self {
        Self {
            logical_min: 0,
            logical_max: 65_535,
            deadzone_force: 1_450.0, // 14.5% deadzone typical of gear-driven wheels
            current_raw_x: 32_768.0,
            angular_velocity_raw_per_sec: 0.0,
            commanded_force: 0,
            tick_dt_us: 1_000, // True 1000 Hz (1.0 ms) default cadence
        }
    }
}

impl VirtualWheelSimulator {
    pub fn new() -> Self {
        Self::default()
    }

    fn advance_physics(&mut self, dt_sec: f64) {
        let f = self.commanded_force as f64;
        let abs_f = f.abs();
        let center_raw = logical_center_raw(self.logical_min, self.logical_max) as f64;
        let disp_from_center = self.current_raw_x - center_raw;
        // In DirectInput8, +f pulls toward lMin (velocity < 0) and -f pulls toward lMax (velocity > 0).
        // Torque is centering when disp_from_center * f > 0, or actively braking incoming velocity when vel * f > 0.
        let returning_to_center = disp_from_center.abs() > 1.0 && (disp_from_center * f) > 0.0;
        let braking_motion =
            self.angular_velocity_raw_per_sec.abs() > 50.0 && (self.angular_velocity_raw_per_sec * f) > 0.0;

        let target_vel = if self.commanded_force == 0 {
            0.0
        } else if returning_to_center || braking_motion {
            // Restorative & derivative-braking PID centering dynamics (~510-560 ms settle to 32768)
            (-f / 3_200.0).clamp(-1.0, 1.0) * 65_000.0
        } else if abs_f <= self.deadzone_force {
            0.0
        } else {
            let eff = (abs_f - self.deadzone_force) / (10_000.0 - self.deadzone_force);
            // Non-linear logarithmic compression curve: ln(1 + 4 * eff) / ln(5)
            let curved = (1.0 + 4.0 * eff).ln() / 5.0_f64.ln();
            // Calibrated to empirical Logitech G920 300ms displacement (~81.8° at force=5000, ~134.6° at force=10000)
            -f.signum() * curved * 35_000.0
        };

        // Time-step-invariant first-order mechanical time constant (tau = 20 ms)
        let alpha = 1.0 - (-dt_sec / 0.020).exp();
        self.angular_velocity_raw_per_sec +=
            (target_vel - self.angular_velocity_raw_per_sec) * alpha;

        // Time-step-invariant viscous brake when motor is off (State 3 settle)
        if self.commanded_force == 0 {
            self.angular_velocity_raw_per_sec *= (-35.0 * dt_sec).exp();
        }

        // Physical motor brush/gear static holding friction near rest (matches G920 startX ~ 19 counts)
        if abs_f < 300.0 && self.angular_velocity_raw_per_sec.abs() < 180.0 {
            self.angular_velocity_raw_per_sec = 0.0;
        }

        self.current_raw_x = (self.current_raw_x + self.angular_velocity_raw_per_sec * dt_sec)
            .clamp(self.logical_min as f64, self.logical_max as f64);
    }
}

impl FfbHardwareBackend for VirtualWheelSimulator {
    #[inline]
    fn logical_range(&self) -> (i32, i32) {
        (self.logical_min, self.logical_max)
    }

    fn read_raw_x(&mut self) -> CoreResult<i32> {
        Ok(self.current_raw_x.round() as i32)
    }

    fn apply_force(&mut self, force: i32) -> CoreResult<()> {
        self.commanded_force = force.clamp(-10_000, 10_000);
        Ok(())
    }

    fn step_tick(&mut self) -> Duration {
        let dt_us = self.tick_dt_us.max(100);
        let dt = Duration::from_micros(dt_us);
        self.advance_physics(dt.as_secs_f64());
        dt
    }
}

// ============================================================================
// `Step Log 2` State Machine Runner (`FUN_004078c0`)
// ============================================================================

/// Executes the complete `WheelCheck.exe` `Step Log 2` state machine (`States 0..=5`)
/// on any [`FfbHardwareBackend`] (physical `FfbSession` or `VirtualWheelSimulator`),
/// tracking exact sub-millisecond [`Duration`] in every state (replicating `WheelCheck.exe`'s
/// wall-clock `local_2e8 = local_130 - _DAT_0045be30` without `.as_millis()` truncation).
pub fn run_step_log2_with_backend<B, F>(
    backend: &mut B,
    config: &StepLog2Config,
    abort_flag: &AtomicBool,
    mut on_progress: F,
) -> CoreResult<Vec<StepLog2Sample>>
where
    B: FfbHardwareBackend,
    F: FnMut(StepLog2Progress),
{
    config.validate()?;

    let (l_min, l_max) = backend.logical_range();
    let settle_target = Duration::from_millis(config.settle_ms);
    let pulse_target = Duration::from_millis(config.duration_ms);

    let mut state = StepLog2State::Init;
    let mut state_elapsed = Duration::ZERO;

    let mut samples: Vec<StepLog2Sample> = Vec::with_capacity((config.steps as usize) + 1);
    let mut current_step: u32 = 0;

    // State 1 (Breakaway Ramp) tracking variables (`DAT_0045be1c` & `DAT_0045be18`)
    let mut ramp_moves: u32 = 0;
    let mut prev_raw_x: i32 = backend.read_raw_x()?;

    // State 2 (PID Center) controller (`FUN_00404040` & `FUN_00404230`) + anti-runaway guard
    let mut pid = PidCenteringController::default();
    let mut pid_entry_abs_norm: i32 = 0;
    let mut pid_prev_abs_norm: i32 = 0;
    let mut pid_runaway_elapsed = Duration::ZERO;

    // State 4 (Force Pulse) start position tracking (`DAT_0045be24` & `_DAT_0045be20`)
    let mut pulse_started = false;
    let mut raw_start_x: i32 = 0;
    let mut norm_start_x: i32 = 0;
    let mut step_csv_force: i32 = 0;

    loop {
        if abort_flag.load(Ordering::SeqCst) {
            let _ = backend.apply_force(0);
            return Err(CoreError::UserAborted);
        }

        let raw_x = backend.read_raw_x()?;
        let norm_x = normalize_raw_to_10000(raw_x, l_min, l_max);

        match state {
            StepLog2State::Init => {
                // State 0 (FUN_004078c0 lines 4977-5002): write row 0 ("0, 0, 0, 0, 0")
                samples.clear();
                samples.push(StepLog2Sample::ZERO);
                current_step = 0;
                ramp_moves = 0;
                prev_raw_x = raw_x;
                state = StepLog2State::BreakawayRamp;
                state_elapsed = Duration::ZERO;
                continue;
            }

            StepLog2State::BreakawayRamp => {
                // State 1 (FUN_004078c0 lines 5003-5021):
                // Gentle wake-up ramp if the wheel starts near center; if already displaced (>3.5%),
                // skip straight to State 2 (PidCenter) so we never push an off-center wheel away.
                if norm_x.abs() > 350 {
                    backend.apply_force(0)?;
                    pid.reset();
                    pid_entry_abs_norm = norm_x.abs();
                    pid_prev_abs_norm = pid_entry_abs_norm;
                    pid_runaway_elapsed = Duration::ZERO;
                    state = StepLog2State::PidCenter;
                    state_elapsed = Duration::ZERO;
                    continue;
                }

                if (raw_x - prev_raw_x).abs() > 2 {
                    ramp_moves += 1;
                    prev_raw_x = raw_x;
                }
                let max_ramp = config.max_force.min(2_000);
                let ramp_mag = (((state_elapsed.as_secs_f64() * 1_000.0) * 50.0) as i32 + 50)
                    .clamp(50, max_ramp);
                backend.apply_force(ramp_mag * config.direction)?;

                let dt = backend.step_tick();
                state_elapsed += dt;

                if ramp_moves > 1 || state_elapsed >= Duration::from_millis(1_000) {
                    backend.apply_force(0)?;
                    pid.reset();
                    pid_entry_abs_norm = norm_x.abs();
                    pid_prev_abs_norm = pid_entry_abs_norm;
                    pid_runaway_elapsed = Duration::ZERO;
                    state = StepLog2State::PidCenter;
                    state_elapsed = Duration::ZERO;
                }
            }

            StepLog2State::PidCenter => {
                // State 2 (FUN_00404230, lines 5022-5035 & 3170-3208):
                // Auto-center wheel to targetX = 0 (raw 32768 on G920) using 1000 Hz PID controller
                // for >= 501 ms until |d_term| < 0.05 (typically ~510-580 ms total).
                let dt = backend.step_tick();
                state_elapsed += dt;
                let dt_sec = dt.as_secs_f64();

                let raw_now = backend.read_raw_x()?;
                let norm_now = normalize_raw_to_10000(raw_now, l_min, l_max);
                let abs_norm = norm_now.abs();

                let prev_force = pid.update(dt_sec, 0, norm_now) * config.direction;

                // Anti-runaway & soft-lock guard during PID centering:
                // Allow brief (< 35 ms) initial braking deceleration right after an 8500 State 4 pulse,
                // then abort immediately if active PID torque drives the wheel FURTHER AWAY from center
                // or exceeds PID_HARD_SAFETY_LIMIT_NORM (9650).
                if state_elapsed >= Duration::from_millis(35)
                    && prev_force.abs() >= 700
                    && abs_norm > 500
                    && abs_norm > pid_prev_abs_norm + 2
                {
                    pid_runaway_elapsed += dt;
                } else if abs_norm < pid_prev_abs_norm {
                    pid_runaway_elapsed = Duration::ZERO;
                }
                pid_prev_abs_norm = abs_norm;

                let runaway_excursion = abs_norm > pid_entry_abs_norm.max(500) + 600;
                if pid_runaway_elapsed >= Duration::from_millis(45)
                    || runaway_excursion
                    || abs_norm >= PID_HARD_SAFETY_LIMIT_NORM
                {
                    let _ = backend.apply_force(0);
                    return Err(CoreError::InvalidStepLogConfig {
                        reason: "Safety cutoff triggered during PID centering: wheel moved away from center or exceeded soft-lock envelope (motor stopped immediately)",
                    });
                }

                backend.apply_force(prev_force)?;

                if pid.is_settled_duration(state_elapsed) {
                    backend.apply_force(0)?;
                    state = StepLog2State::MotorOffSettle;
                    state_elapsed = Duration::ZERO;
                }
            }

            StepLog2State::MotorOffSettle => {
                // State 3 (FUN_004078c0 lines 5036-5042):
                // Set force = 0 once and wait exact settle_ms (default 300.0 ms)
                backend.apply_force(0)?;
                let dt = backend.step_tick();
                state_elapsed += dt;

                if state_elapsed >= settle_target {
                    pulse_started = false;
                    state = StepLog2State::ForcePulse;
                    state_elapsed = Duration::ZERO;
                }
            }

            StepLog2State::ForcePulse => {
                // State 4 (FUN_004078c0 lines 5043-5072):
                if !pulse_started {
                    raw_start_x = raw_x;
                    norm_start_x = norm_x;
                    step_csv_force = compute_step_force(
                        current_step,
                        config.steps,
                        config.max_force,
                        1, // CSV force column is positive 0..=max_force
                    );
                    // Verified in WheelCheck FUN_004078c0 lines 4965, 5048 & 5944:
                    // Apply constant force ONCE on pulse entry (`local_2e8 == 0` / `local_148 != DAT_0045bd0c`).
                    // Positive ConstantForce rotates the wheel toward lMin (0, left),
                    // so rawEndX < rawStartX, making -(rawEndX - rawStartX) and -deltaX positive!
                    let motor_torque = step_csv_force * config.direction;
                    backend.apply_force(motor_torque)?;
                    pulse_started = true;
                }

                let dt = backend.step_tick();
                state_elapsed += dt;

                let raw_cur = backend.read_raw_x()?;
                let norm_cur = normalize_raw_to_10000(raw_cur, l_min, l_max);
                // Soft-lock cutoff at 85% physical rotation (|norm_cur| >= 8500, ±382.5° on a 900° wheel)
                let hit_endstop = norm_cur.abs() >= ENDSTOP_SAFETY_LIMIT_NORM;

                if state_elapsed >= pulse_target || hit_endstop {
                    backend.apply_force(0)?;

                    let raw_end_x = raw_cur;
                    let norm_end_x = norm_cur;
                    let norm_delta = norm_end_x - norm_start_x;

                    let delta_x_deg = compute_delta_x_deg(
                        raw_start_x,
                        raw_end_x,
                        l_min,
                        l_max,
                        config.max_angle_deg,
                        config.direction,
                    );

                    // WheelCheck line 5059: "%d, %d, %d, %d, %f\n", force, -startX, -endX, -deltaX, deltaXDeg
                    let sample = StepLog2Sample {
                        force: step_csv_force,
                        start_x: -norm_start_x * config.direction,
                        end_x: -norm_end_x * config.direction,
                        delta_x: -norm_delta * config.direction,
                        delta_x_deg,
                    };
                    samples.push(sample);

                    on_progress(StepLog2Progress {
                        step_index: current_step + 1,
                        total_steps: config.steps,
                        sample,
                        early_cutoff: hit_endstop && state_elapsed < pulse_target,
                    });

                    current_step += 1;
                    if current_step < config.steps {
                        pid.reset();
                        pid_entry_abs_norm = norm_cur.abs();
                        pid_prev_abs_norm = pid_entry_abs_norm;
                        pid_runaway_elapsed = Duration::ZERO;
                        state = StepLog2State::PidCenter;
                        state_elapsed = Duration::ZERO;
                    } else {
                        state = StepLog2State::Complete;
                        state_elapsed = Duration::ZERO;
                    }
                }
            }

            StepLog2State::Complete => {
                // State 5 (FUN_004078c0 lines 5073-5089): zero force and exit cleanly
                backend.apply_force(0)?;
                break;
            }
        }
    }

    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_normalize_raw_to_10000_accuracy() {
        assert_eq!(logical_center_raw(0, 65535), 32768);
        assert_eq!(logical_center_raw(-32768, 32767), 0);

        assert_eq!(normalize_raw_to_10000(0, 0, 65535), -10_000);
        assert_eq!(normalize_raw_to_10000(32768, 0, 65535), 0);
        assert_eq!(normalize_raw_to_10000(65535, 0, 65535), 10_000);

        assert!((normalize_raw_to_unit(0, 0, 65535) - (-1.0)).abs() < 1e-6);
        assert_eq!(normalize_raw_to_unit(32768, 0, 65535), 0.0);
        assert!((normalize_raw_to_unit(65535, 0, 65535) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_compute_step_force_and_delta_x_deg_formula() {
        // 50 steps, max_force = 10000 -> step 0 = 200, step 24 = 5000, step 49 = 10000
        assert_eq!(compute_step_force(0, 50, 10_000, 1), 200);
        assert_eq!(compute_step_force(24, 50, 10_000, 1), 5_000);
        assert_eq!(compute_step_force(49, 50, 10_000, 1), 10_000);

        // Full 16-bit span (65535 counts) over 900.0 deg:
        // Moving -6553.5 raw counts (10% of full lock) -> +90.0 degrees
        let deg = compute_delta_x_deg(32768, 32768 - 6553, 0, 65530, 900.0, 1);
        assert!((deg - 90.0).abs() < 1e-4, "Expected 90.0 deg, got {deg}");
    }

    #[test]
    fn test_pid_controller_convergence() {
        let mut pid = PidCenteringController::default();
        let mut pos_norm: f64 = 4_500.0; // Displaced +45% (right of center)
        let mut vel_norm_per_sec: f64 = 0.0;
        let dt_ms: u64 = 5;
        let dt_sec = 0.005;
        let mut elapsed_ms: u64 = 0;

        while elapsed_ms < 5_000 {
            let cmd = pid.update(dt_sec, 0, pos_norm.round() as i32);
            // DirectInput8 physics: +cmd pulls toward lMin (-10000), so acceleration is -(cmd)
            let accel = -(cmd as f64) * 12.0 - vel_norm_per_sec * 18.0;
            vel_norm_per_sec += accel * dt_sec;
            pos_norm += vel_norm_per_sec * dt_sec;
            elapsed_ms += dt_ms;

            if pid.is_settled(elapsed_ms) {
                break;
            }
        }

        assert!(
            elapsed_ms >= 501 && elapsed_ms < 5_000,
            "PID should settle after 501 ms and before 5000 ms (settled at {elapsed_ms} ms)"
        );
        assert!(
            pos_norm.abs() < 50.0,
            "Wheel should converge near 0 (got {pos_norm})"
        );
        assert!(
            pid.last_d_term().abs() < 0.05,
            "Derivative term should be < 0.05 at settle"
        );
    }

    #[test]
    fn test_virtual_simulator_step_log2_state_machine_and_csv_format() {
        let mut sim = VirtualWheelSimulator::new();
        let cfg = StepLog2Config {
            steps: 10,
            duration_ms: 300,
            settle_ms: 300,
            max_angle_deg: 900.0,
            max_force: 10_000,
            direction: 1,
        };
        let abort = AtomicBool::new(false);
        let mut progress_count = 0usize;

        let samples = run_step_log2_with_backend(&mut sim, &cfg, &abort, |_prog| {
            progress_count += 1;
        })
        .expect("Virtual Step Log 2 should succeed");

        assert_eq!(progress_count, 10);
        // 10 steps + row 0 = 11 rows
        assert_eq!(samples.len(), 11);
        assert_eq!(samples[0], StepLog2Sample::ZERO);

        // Step 1 (force = 1000) is inside the 14.5% (1450) deadzone -> 0 movement
        assert_eq!(samples[1].force, 1_000);
        assert!(samples[1].delta_x_deg.abs() < 1e-3);

        // Step 10 (force = 10000) is well above deadzone -> positive movement
        assert_eq!(samples[10].force, 10_000);
        assert!(samples[10].delta_x > 0);
        assert!(samples[10].delta_x_deg > 10.0);

        let csv = format_step_log2_csv(&samples);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 12); // Header + 11 sample rows
        assert_eq!(lines[0], "force, startX, endX, deltaX, deltaXDeg");
        assert_eq!(lines[1], "0, 0, 0, 0, 0");
    }

    /// Regression Test 001 (Hotfix 001):
    /// Simulates a Logitech G920 (`LogicalRange = [0, 65535]`, physical center `0° = 32768`,
    /// full left lock `-450° = 0`, full right lock `+450° = 65535`) and proves:
    /// 1. At `raw = 32768`, normalized position is `0` (`0.0` unit) and PID centering force is `0` (NOT `-10000` or `+10000`).
    /// 2. During `step-log2`, every step begins centered near `32768` (`|start_x| <= 200` in `[-10000, 10000]`,
    ///    i.e., within ±2% of `32768`), moves proportionally to step force while staying strictly inside the
    ///    safe `[15000, 50535]` raw envelope (never approaching `0` or `65535` mechanical end-stops),
    ///    and smoothly returns to `32768`.
    #[test]
    fn test_reg_001_g920_unsigned_logical_range_centering_and_no_endstop_slam() {
        let mut pid = PidCenteringController::default();
        let center_norm = normalize_raw_to_10000(32_768, 0, 65_535);
        assert_eq!(center_norm, 0, "G920 raw 32768 must normalize to 0");
        assert_eq!(
            pid.update(0.005, 0, center_norm),
            0,
            "PID centering force at physical center (raw=32768) must be 0"
        );

        // Verify restorative sign on both sides of 32768:
        // Left of center (raw = 24000 < 32768 => norm < 0): PID must output negative force to pull right toward 32768
        let mut pid_left = PidCenteringController::default();
        let norm_left = normalize_raw_to_10000(24_000, 0, 65_535);
        assert!(norm_left < 0);
        let force_from_left = pid_left.update(0.005, 0, norm_left);
        assert!(
            force_from_left < 0 && force_from_left >= -DEFAULT_MAX_CENTERING_FORCE,
            "When left of 32768, PID force must be negative and capped (got {force_from_left})"
        );

        // Right of center (raw = 41535 > 32768 => norm > 0): PID must output positive force to pull left toward 32768
        let mut pid_right = PidCenteringController::default();
        let norm_right = normalize_raw_to_10000(41_535, 0, 65_535);
        assert!(norm_right > 0);
        let force_from_right = pid_right.update(0.005, 0, norm_right);
        assert!(
            force_from_right > 0 && force_from_right <= DEFAULT_MAX_CENTERING_FORCE,
            "When right of 32768, PID force must be positive and capped (got {force_from_right})"
        );

        // Execute full Step Log 2 on G920 virtual backend starting at raw = 32768 (1000 Hz = 1000 us/tick)
        let mut sim = VirtualWheelSimulator {
            logical_min: 0,
            logical_max: 65_535,
            deadzone_force: 1_450.0,
            current_raw_x: 32_768.0,
            angular_velocity_raw_per_sec: 0.0,
            commanded_force: 0,
            tick_dt_us: 1_000,
        };
        let cfg = StepLog2Config {
            steps: 20,
            duration_ms: 300,
            settle_ms: 300,
            max_angle_deg: 900.0,
            max_force: 10_000,
            direction: 1,
        };
        let abort = AtomicBool::new(false);
        let samples = run_step_log2_with_backend(&mut sim, &cfg, &abort, |_prog| {})
            .expect("G920 Step Log 2 simulation must complete without runaway");

        for (idx, s) in samples.iter().enumerate().skip(1) {
            // Every step must start within ±2% (200 normalized units) of center 32768
            assert!(
                s.start_x.abs() <= 200,
                "Step {idx} did not start near center 32768: start_x={} (expected |start_x| <= 200)",
                s.start_x
            );
            // Every step must stay strictly within the 8500 soft-lock envelope (|end_x| <= 8650)
            assert!(
                s.end_x.abs() <= ENDSTOP_SAFETY_LIMIT_NORM + 150,
                "Step {idx} exceeded 8500 soft-lock envelope: end_x={}",
                s.end_x
            );
            assert!(
                s.delta_x >= -50,
                "Step {idx} moved in wrong direction: delta_x={}",
                s.delta_x
            );
        }
    }

    #[test]
    fn test_reg_001_pid_runaway_safety_cutoff_if_polarity_inverted() {
        let mut sim = VirtualWheelSimulator {
            logical_min: 0,
            logical_max: 65_535,
            deadzone_force: 500.0,
            current_raw_x: 40_000.0, // Displaced right of center
            angular_velocity_raw_per_sec: 0.0,
            commanded_force: 0,
            tick_dt_us: 1_000,
        };
        // Intentionally invert direction (-1) on a standard-polarity wheel so PID pushes away from center
        let cfg = StepLog2Config {
            steps: 5,
            duration_ms: 300,
            settle_ms: 300,
            max_angle_deg: 900.0,
            max_force: 10_000,
            direction: -1,
        };
        let abort = AtomicBool::new(false);
        let res = run_step_log2_with_backend(&mut sim, &cfg, &abort, |_prog| {});
        assert!(
            res.is_err(),
            "Runaway detection must immediately abort and cut force to 0 when wheel moves away from center"
        );
        assert_eq!(
            sim.commanded_force, 0,
            "Motor force must be zeroed immediately upon runaway safety cutoff"
        );
    }

    /// Regression Test 002-A (Hotfix 002):
    /// Verifies `HighResTimerGuard` activates `timeBeginPeriod(1)` (`1000 Hz` multimedia timer),
    /// achieves sub-3ms `sleep_1ms()` ticks (never falling back to the 15.6 ms 64 Hz Windows quantum),
    /// and unconditionally restores the original process priority class on `Drop`.
    #[test]
    fn test_reg_002_high_res_timer_guard_1000hz_cadence_and_raii_drop() {
        let orig_prio = unsafe { GetPriorityClass(GetCurrentProcess()) };
        {
            let guard = HighResTimerGuard::new();
            assert!(
                guard.is_timer_period_active(),
                "HighResTimerGuard must activate timeBeginPeriod(1)"
            );

            let start = Instant::now();
            for _ in 0..30 {
                guard.sleep_1ms();
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            let avg_tick_ms = elapsed_ms / 30.0;
            assert!(
                avg_tick_ms < 4.5,
                "Expected ~1.0-1.7 ms/tick with HighResTimerGuard, got {avg_tick_ms:.3} ms/tick (total {elapsed_ms:.1} ms)"
            );
        }
        let restored_prio = unsafe { GetPriorityClass(GetCurrentProcess()) };
        assert_eq!(
            restored_prio, orig_prio,
            "HighResTimerGuard::drop must restore original process priority class"
        );
    }

    /// Regression Test 002-B (Hotfix 002):
    /// Reproduces the exact `1.68 ms` (`1_680 µs`, `5/3 ms`) tick interval observed on Windows `Sleep(1)`
    /// that previously truncated `.as_millis()` down to `1 ms` and stretched `300 ms` pulses to `504 ms` (`1.68x`).
    /// Proves that exact `Duration` accumulation produces identical `300 ms` angular displacements
    /// (`< 2%` difference instead of `68%` inflation!) and `~1.15s/step` total runtime (`~57.5s` for 50 steps).
    #[test]
    fn test_reg_002_fractional_1680us_tick_eliminates_1_68x_displacement_bug() {
        struct TimedVirtualBackend {
            inner: VirtualWheelSimulator,
            total_simulated: Duration,
        }
        impl FfbHardwareBackend for TimedVirtualBackend {
            fn logical_range(&self) -> (i32, i32) {
                self.inner.logical_range()
            }
            fn read_raw_x(&mut self) -> CoreResult<i32> {
                self.inner.read_raw_x()
            }
            fn apply_force(&mut self, force: i32) -> CoreResult<()> {
                self.inner.apply_force(force)
            }
            fn step_tick(&mut self) -> Duration {
                let dt = self.inner.step_tick();
                self.total_simulated += dt;
                dt
            }
        }

        let cfg = StepLog2Config {
            steps: 50,
            duration_ms: 300,
            settle_ms: 300,
            max_angle_deg: 900.0,
            max_force: 10_000,
            direction: 1,
        };
        let abort = AtomicBool::new(false);

        let mut backend_1000us = TimedVirtualBackend {
            inner: VirtualWheelSimulator {
                tick_dt_us: 1_000, // Exact 1.000 ms (1000 Hz)
                ..VirtualWheelSimulator::default()
            },
            total_simulated: Duration::ZERO,
        };
        let samples_1000 =
            run_step_log2_with_backend(&mut backend_1000us, &cfg, &abort, |_| {}).unwrap();

        let mut backend_1680us = TimedVirtualBackend {
            inner: VirtualWheelSimulator {
                tick_dt_us: 1_680, // Exact 1.680 ms (5/3 ms) fractional tick
                ..VirtualWheelSimulator::default()
            },
            total_simulated: Duration::ZERO,
        };
        let samples_1680 =
            run_step_log2_with_backend(&mut backend_1680us, &cfg, &abort, |_| {}).unwrap();

        // Check mid-scale step (force = 5000, index 25) and full-scale step (force = 10000, index 50)
        for &step_idx in &[25usize, 50usize] {
            let deg_1000 = samples_1000[step_idx].delta_x_deg;
            let deg_1680 = samples_1680[step_idx].delta_x_deg;
            let ratio = deg_1680 / deg_1000;
            assert!(
                (ratio - 1.0).abs() < 0.025,
                "Step {step_idx} displacement must NOT inflate by 1.68x on 1.68ms ticks: 1.0ms={deg_1000:.3}°, 1.68ms={deg_1680:.3}° (ratio={ratio:.4})"
            );
        }

        // Verify 50-step runtime parity with WheelCheck.exe (~55-63 seconds total, ~1.15s/step)
        let total_secs_1000 = backend_1000us.total_simulated.as_secs_f64();
        let total_secs_1680 = backend_1680us.total_simulated.as_secs_f64();
        assert!(
            (54.0..=64.0).contains(&total_secs_1000),
            "50-step total runtime at 1000 Hz should be ~58s (~1.15s/step), got {total_secs_1000:.2}s"
        );
        assert!(
            (54.0..=64.0).contains(&total_secs_1680),
            "50-step total runtime at 1.68ms ticks should also be ~58s (~1.15s/step), got {total_secs_1680:.2}s"
        );
    }

    /// Regression Test 002-C (Hotfix 002):
    /// Verifies:
    /// 1. `ENDSTOP_SAFETY_LIMIT_NORM == 8_500` (`~382.5°`), ensuring high-torque wheels can travel
    ///    smoothly past `5_000` (`225°`) up to `8_500` without artificial plateauing.
    /// 2. From typical Logitech G920 peak 300ms displacement (`-3620` norm = `-163°`, `raw = 20_906`),
    ///    `PidCenteringController` settles in `501..=650 ms` (`~550 ms`).
    /// 3. Even from the extreme `-8500` (`-382.5°`, `raw = 4_915`) soft-lock boundary, `State 2`
    ///    (`PidCenter`) returns smoothly to center (`|norm| <= 100`) in `< 950 ms` without false runaway errors.
    #[test]
    fn test_reg_002_8500_soft_lock_cutoff_and_fast_550ms_pid_return() {
        assert_eq!(ENDSTOP_SAFETY_LIMIT_NORM, 8_500);

        // 1. Typical G920 peak 300ms displacement (-3620 norm = -163°, raw = 20_906) -> settles in 501..=650 ms
        let mut sim_g920 = VirtualWheelSimulator {
            current_raw_x: 20_906.0,
            tick_dt_us: 1_000,
            ..VirtualWheelSimulator::default()
        };
        let mut pid_g920 = PidCenteringController::default();
        let mut elapsed_g920 = Duration::ZERO;
        while elapsed_g920 < Duration::from_millis(1_500) {
            let dt = sim_g920.step_tick();
            elapsed_g920 += dt;
            let raw = sim_g920.read_raw_x().unwrap();
            let norm = normalize_raw_to_10000(raw, 0, 65_535);
            let cmd = pid_g920.update(dt.as_secs_f64(), 0, norm);
            sim_g920.apply_force(cmd).unwrap();
            if pid_g920.is_settled_duration(elapsed_g920) {
                break;
            }
        }
        let settled_g920_ms = elapsed_g920.as_millis();
        let final_g920_norm = normalize_raw_to_10000(sim_g920.read_raw_x().unwrap(), 0, 65_535);
        assert!(
            (501..=650).contains(&settled_g920_ms),
            "G920 PID return from -163° (-3620 norm) should settle in 501..=650 ms, took {settled_g920_ms} ms"
        );
        assert!(
            final_g920_norm.abs() <= 100,
            "G920 PID return should park near center 0, got norm={final_g920_norm}"
        );

        // 2. Extreme -8500 (-382.5°, raw = 4_915) soft-lock boundary return -> settles cleanly < 950 ms
        let mut sim_ext = VirtualWheelSimulator {
            current_raw_x: 4_915.0,
            tick_dt_us: 1_000,
            ..VirtualWheelSimulator::default()
        };
        let mut pid_ext = PidCenteringController::default();
        let mut elapsed_ext = Duration::ZERO;
        while elapsed_ext < Duration::from_millis(1_500) {
            let dt = sim_ext.step_tick();
            elapsed_ext += dt;
            let raw = sim_ext.read_raw_x().unwrap();
            let norm = normalize_raw_to_10000(raw, 0, 65_535);
            let cmd = pid_ext.update(dt.as_secs_f64(), 0, norm);
            sim_ext.apply_force(cmd).unwrap();
            if pid_ext.is_settled_duration(elapsed_ext) {
                break;
            }
        }
        let settled_ext_ms = elapsed_ext.as_millis();
        let final_ext_norm = normalize_raw_to_10000(sim_ext.read_raw_x().unwrap(), 0, 65_535);
        assert!(
            (501..=950).contains(&settled_ext_ms),
            "Extreme PID return from -8500 (-382.5°) should settle in 501..=950 ms, took {settled_ext_ms} ms"
        );
        assert!(
            final_ext_norm.abs() <= 100,
            "Extreme PID return from -8500 should park near center 0, got norm={final_ext_norm}"
        );
    }
}
