# Session Log — 2026-10-27: Phase 2.2 Force Feedback Engine, PID Auto-Centering & WheelCheck Step Log 2

## 1. Objective
Implement the complete `WheelCheck.exe` `Step Log 2` Force Feedback engine (`ffb_engine.rs`) in `speed-precise-core` and expose the `step-log2` subcommand in `speed-precise-cli`, replicating `FUN_004034a0` (device FFB init), `FUN_00404040` / `FUN_00404230` (PID auto-centering), and `FUN_004078c0` (`Step Log 2` 6-state machine and `deltaXDeg` math), paired with RAII motor safety guards (`FfbSession` `Drop`) and a deterministic virtual wheel simulator (`--dry-run`).

## 2. Files Created / Modified
- `crates/speed-precise-core/src/dinput_ffi.rs` — Added `DiConstantForce` (`4 B`), `DiEnvelope` (`20 B`), `DiEffect` (`80 B` on x64, `56 B` on x86), `IDirectInputEffectVtbl`, FFB flags (`DIEFF_*`, `DIEP_*`, `DISFFC_*`), and `user32.dll` / `kernel32.dll` window helpers (`CreateWindowExW`, `DestroyWindow`, `GetConsoleWindow`, `GetForegroundWindow`, `GetDesktopWindow`).
- `crates/speed-precise-core/src/ffb_engine.rs` — Implemented `FfbSession` (RAII `Drop` safety guard + `FfbWindowGuard`), `PidCenteringController` (`Kp=5.0, Ki=0.0, Kd=0.25, Td=0.9`), `VirtualWheelSimulator` (14.5% deadzone + logarithmic response), `run_step_log2_with_backend` (States `0..=5`), `compute_delta_x_deg`, and `write_step_log2_csv`.
- `crates/speed-precise-core/src/error.rs` & `src/lib.rs` — Added FFB/CSV/Abort errors and exported `ffb_engine` public symbols.
- `crates/speed-precise-cli/src/main.rs` — Added `step-log2` subcommand with live step progress and `ctrlc` abort handler.
- `run_regression_tests.py` — Added `step-log2 --dry-run --steps 10` end-to-end smoke test and CSV format verifier.
- `dev_wiki/INDEX.md` & `dev_wiki/KNOWN_TRAPS.md`.

## 3. Verification Results
- `python run_regression_tests.py` passed 6/6 checks with exit code `0`:
  - `cargo check --workspace --all-targets`: 0 errors, 0 warnings.
  - `cargo test --workspace`: 19/19 unit tests passed (16 in `speed-precise-core`, 3 in `speed-precise-cli`).
  - `step-log2 --dry-run --steps 10`: Generated and verified 12-line CSV matching exact `WheelCheck.exe` `log2.csv` format.
