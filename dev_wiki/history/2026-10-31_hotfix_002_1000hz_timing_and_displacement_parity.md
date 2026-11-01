# Task Log — Hotfix & Regression 002: Exact Timing Speed Parity (`1000 Hz` Timer), `1.68x` Displacement Fix & `8500` Soft-Lock

- **Date:** 2026-10-31
- **Phase:** Phase 2 Hardware Validation Hotfix 002 (`speed-precise-core` & `speed-precise-cli`)
- **Status:** COMPLETED & VERIFIED (`7/7` Regression Checks, `31/31` Unit Tests Passing, `0` Warnings)

## 1. Empirical Hardware Findings Audited (Real Logitech G920 `046D:C262`)
Cross-referencing `log2-speedprecise.csv` against `WheelCheck.exe` (`log2- original-mola-desligada.csv` and decompiled `FUN_00401210`, `FUN_004024d0`, `FUN_004034a0`, `FUN_00404040`, `FUN_00404230`, `FUN_004078c0`) plus live Windows kernel timing benchmarks on the host OS revealed the exact physical and software root causes of all three discrepancies:
1. **Symptom 1 (`~2x` Longer Total Test Duration: `~120s` vs `~58s` for 50 Steps):**
   - Missing `timeBeginPeriod(1)` / `timeEndPeriod(1)` (`winmm.dll`), `SetPriorityClass` (`HIGH_PRIORITY_CLASS` / `REALTIME_PRIORITY_CLASS`), and high-resolution timer pacing (`FUN_004024d0` & `FUN_00401210`).
   - In Hotfix 001, `PidCenteringController` capped `DEFAULT_MAX_CENTERING_FORCE = 2_800` (`28%`) and applied a `120 ms` torque ramp-in, stalling return-to-center to `~1.4s/step` instead of `WheelCheck.exe`'s `~550 ms/step`, compounded by `.as_millis()` truncation stretching `State 2`, `State 3`, and `State 4` durations by `1.68x`.
2. **Symptom 2 (`1.68x` / `5/3` Larger Angular Displacement in `State 4`: `0.0274 deg/force` vs `0.0163 deg/force` with `R² = 0.9997`):**
   - Benchmarked directly on the host Windows kernel: Win32 `Sleep(1)` (`std::thread::sleep(Duration::from_millis(1))`) sleeps across two `1 ms` clock edges (`500.20 ms` for `300` calls = **`1.667 ms/tick` = `5/3 ms`**, and **`1.680 ms/tick`** including `Poll()` + `GetDeviceState()`).
   - `FfbSession::step_tick_ms()` previously called `now.duration_since(self.last_tick).as_millis()`, which **truncated `1.680 ms` down to integer `1 ms`** and immediately overwrote `self.last_tick = now`, permanently discarding `0.680 ms` on every tick!
   - Consequently, `State 4` (`state_elapsed_ms >= 300`) kept motor torque energized for **`300 * 1.680 ms = 504 ms`** instead of `300 ms`, scaling terminal-velocity angular displacement across all steps by **`504 / 300 = 1.68x` (`5/3`)** while keeping normalized curve shape identical (`R² = 0.9997`). By contrast, `WheelCheck.exe` (`FUN_004078c0` line `4975`) computes wall-clock elapsed time directly (`local_2e8 = timeGetTime() - _DAT_0045be30`).
3. **Symptom 3 (Artificial Plateau at `endX = 5000` / `224.5°` for `force = 8200..10000`):**
   - `ENDSTOP_SAFETY_LIMIT_NORM = 5_000` cut off high-force pulses at `50%` half-travel (`225°`).

## 2. Code Changes Implemented
- **`crates/speed-precise-core/src/dinput_ffi.rs`:**
  - Added FFI bindings for `winmm.dll` (`timeBeginPeriod`, `timeEndPeriod`) and `kernel32.dll` (`GetCurrentProcess`, `GetPriorityClass`, `SetPriorityClass`, `GetCurrentThread`, `SetThreadAffinityMask`, `CreateWaitableTimerExW`, `SetWaitableTimer`, `WaitForSingleObject`, `CloseHandle`, `Sleep`) plus unit test `test_winmm_timer_and_kernel32_priority_ffi`.
- **`crates/speed-precise-core/src/ffb_engine.rs`:**
  - Implemented RAII [`HighResTimerGuard`] activating `timeBeginPeriod(1)`, `SetPriorityClass` (`REALTIME_PRIORITY_CLASS` with fallback to `HIGH_PRIORITY_CLASS`), `SetThreadAffinityMask(1)`, and `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` (`~1.02 ms/tick = ~1000 Hz`, zero busy-spin) with unconditional restoration in `Drop`.
  - Replaced truncated integer `step_tick_ms() -> u64` with exact sub-millisecond `FfbHardwareBackend::step_tick(&mut self) -> Duration`, accumulating exact `Duration` in `run_step_log2_with_backend` so `State 3` (`300.0 ms`) and `State 4` (`300.0 ms`) never suffer fractional millisecond loss.
  - Updated `FfbSession::open` to set `DIPROP_BUFFERSIZE = 10` while unacquired (`FUN_004034a0` line `2770`) and updated `FfbSession::apply_force` to call `Start(INFINITE, 0)` once and deduplicate unchanged forces via `last_applied_force: Option<i32>` (`FUN_004078c0` lines `5931–6020`, `local_148 != DAT_0045bd0c` with `DIEP_TYPESPECIFICPARAMS`).
  - Restored full `1000 Hz` `PidCenteringController` response (`DEFAULT_MAX_CENTERING_FORCE = 10_000`, `dt` EMA smoothing `0.1 * raw_dt + 0.9 * prev_dt`, no artificial `120 ms` deadzone stall) for `~520–580 ms` centering per step.
  - Raised `ENDSTOP_SAFETY_LIMIT_NORM` from `5_000` to `8_500` (`~382.5°`) and `PID_HARD_SAFETY_LIMIT_NORM` to `9_650`.
  - Added 3 regression tests: `test_reg_002_high_res_timer_guard_1000hz_cadence_and_raii_drop`, `test_reg_002_fractional_1680us_tick_eliminates_1_68x_displacement_bug`, and `test_reg_002_8500_soft_lock_cutoff_and_fast_550ms_pid_return`.
- **`crates/speed-precise-cli/src/main.rs` & `run_regression_tests.py`:**
  - Updated CLI safety banner (`1000 Hz High-Res Timer`, `|X_norm| >= 8500`), added wall-clock total & per-step timing summary, and updated CSV soft-lock regression check to `8650`.
