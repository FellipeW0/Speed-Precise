# Task Report — Phase 3.2: Clean UX Redesign, Vertical Pedal Dashboard, Smart Saturation Math & Unified Auto-LUT

- **Date:** 2026-10-31
- **Status:** Completed & Verified (`10/10` regression suite stages passed, `41/41` unit tests passed, `0` warnings under `-D warnings`)

## 1. Objective
Redesign `speed-precise-gui` from a 3-tab technical debugger into a clean, consumer-friendly **2-Screen Sim Racing Dashboard**, hiding raw DirectInput8 debug codes (`dwType`, `0x0B`, `VID:PID`, raw integer ranges), removing the permanent red Emergency Stop button from the top header (making it strictly contextual during active FFB execution), implementing automatic `[lMin, lCenter, lMax]` recalculation for pedal saturation on inverted Logitech G920 and generic pedals, rendering a cockpit-accurate layout (Horizontal Steering + 3 Vertical Pedal Columns: Clutch, Brake, Accelerator), unifying `Step Log 2` + `LUT Generator` into a 1-click Auto-LUT screen, and strictly locking the LUT chart zoom/pan inside `[0.0, 1.0] × [0.0, 1.0]`.

## 2. Changes Implemented
- **`crates/speed-precise-core/src/calibration.rs`**:
  - Implemented `compute_smart_pedal_calibration(logical_min, logical_max, saturation_pct, deadzone_pct, inverted)` to compute `[lMin, lCenter, lMax]` directly for `DIPROP_CALIBRATION` (`0x0B`) instead of relying on DirectInput's center-symmetric `DIPROP_SATURATION`.
  - Verified exact Logitech G920 (`0..=255`, `inverted = true`, resting = `255`, full press = `0`) math: `40%` saturation (`0%` deadzone) -> `[153, 204, 255]`; `100%` saturation (`0%` deadzone) -> `[0, 127, 255]`.
  - Implemented `infer_pedal_sliders_from_calibration` (so existing Registry calibrations such as `[73, 161, 255]` initialize sliders accurately at `~71.4%` without overwriting user presets on startup), `compute_pedal_press_percentages` (`0%` bottom at rest -> `100%` top at full press for both physical and in-game vertical bars), and `compute_steering_angle_deg`.
- **`crates/speed-precise-core/src/device.rs` & `lib.rs`**:
  - Added `CockpitAxisMapping::detect` and `default_pedal_inverted` (`VID == 0x046D` and upper-anchored calibration detection) to auto-assign Steering (`Wheel axis` / `X Axis`), Clutch (`Clutch` / `Slider`), Brake (`Brake` / `Rz Axis`), and Accelerator (`Accelerator` / `Y Axis`).
- **`crates/speed-precise-gui/src/worker.rs`**:
  - Replaced technical status/phase strings (`0x0B`, `046D:C262`, `[#0]`, `--dry-run`, `PID Centering`, `Settle 300ms`) with clean PT-BR consumer messages while preserving `< 1 ms` `Arc<AtomicBool>` cutoff and RAII `FfbSession` safety guarantees.
- **`crates/speed-precise-gui/src/app.rs`**:
  - Replaced `ActiveTab` (3 tabs) with `ActiveScreen` (2 screens: `CockpitDashboard` — *"Volante e Pedais"*, and `AutoLutStudio` — *"Gerador de LUT (Force Feedback)"*).
  - Removed the permanent red Emergency Stop button from the top header; rendered `[⛔ Parar Teste Agora (ESPAÇO / ESC)]` **only** while `ffb_running == true` next to the live `Passo X de 50` progress bar (while keeping `Space`/`ESC` hotkeys active during FFB tests).
  - Built custom `egui::Painter` widgets for the horizontal center-zero steering bar (`paint_horizontal_steering_gauge`) and the 3 side-by-side vertical dual-bar pedal columns (`paint_vertical_pedal_bars`: `1. Embreagem` Left, `2. Freio` Center, `3. Acelerador` Right).
  - Merged `Step Log 2` and `LUT Generator` into a single 1-click Auto-LUT workflow that immediately computes and displays the final 3-curve LUT chart upon test completion, locked strictly within `[0.0, 1.0] × [0.0, 1.0]` via `clamp_unit_square_bounds` and `zoom_unit_square_bounds` + `[🔍 Resetar Zoom]`.
- **`run_regression_tests.py`**:
  - Upgraded master regression suite to 10 stages including `cargo build --release -p speed-precise-gui` and verification of both `debug` and `release` `speed-precise-gui.exe` binaries.

## 3. Verification Results
- `python run_regression_tests.py`: **`10/10` stages PASSED** (`0` warnings with `RUSTFLAGS="-D warnings"`).
- Unit tests: **`41/41` PASSED** (`speed-precise-core`: `31`, `speed-precise-cli`: `4`, `speed-precise-gui`: `6`).
- Compiled release binary: `target\release\speed-precise-gui.exe` (`5006.5 KiB`).
