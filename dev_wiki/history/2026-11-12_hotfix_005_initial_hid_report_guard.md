# 2026-11-12 — Hotfix 005: Initial DirectInput8 HID Report Guard (Eliminating `50%` / `91%` Phantom Startup Values)

## Summary
Eliminated phantom startup pedal readings (`Embreagem 50%`, `Freio 91%`, `Acelerador 50%` with `Físico: 50%`) that appeared immediately after controller recognition until the user touched a pedal on the connected **Logitech G HUB G920 (`046D:C262`)**.

## Empirical Hardware Diagnosis
Live diagnostics against the attached Logitech G HUB G920 (`046D:C262`) verified:
- Logitech G HUB's virtual HID miniport sends **no** HID interrupt packets while the hardware sits completely untouched (`ReadFile` on `col01` times out with `WAIT_TIMEOUT`, `HidD_GetInputReport(rid=1)` fails with `ERROR_GEN_FAILURE = 31`, and `winmm::joyGetPosEx` returns zeros).
- Bare `IDirectInputDevice8W::Acquire()` + `Poll()` + `GetDeviceState()` (even with zero `SetProperty` calls) initializes `dinput8.dll`'s axis state buffer to the range midpoint across every axis until the first USB HID interrupt packet arrives:
  - `p_raw_device` (`DIPROPCALIBRATIONMODE_RAW = 1`): `Wheel axis = 32767`, `Clutch = 127`, `Brake = 127`, `Accelerator = 127`
  - `p_cooked_device` (`COOKED = 0`): `32768` (`50.0%`) across all 4 axes
- For inverted Logitech pedals (`0..=255`, rest = `255`), raw `127` reads as `Físico: 50%`, and when evaluated against a saved Brake calibration (`55%` saturation -> `[115, 185, 255]`), `127` (`50%`) scales to **`91%`**, appearing as random/stuck values until a single pedal press triggers a `ReportID = 1` packet containing all axis positions.

## Changes Applied
- **`crates/speed-precise-core/src/device.rs`:**
  - Extended `DualDeviceHandle` with `mapping`, `awaiting_initial_hid_report`, and `initial_probe_raw` (plus public getter `has_received_initial_hid_report()`).
  - `DualDeviceHandle::open` now probes the initial post-`Acquire()` buffer and checks `all_axes_at_dinput_midpoint(&info.axes, &raw_state_buf, &cooked_state_buf)`.
  - Added pure helpers `all_axes_at_dinput_midpoint`, `detect_initial_hid_report_arrival`, `axis_uninitialized_resting_raw`, and `resolve_axis_telemetry_with_initial_guard`.
  - While `awaiting_initial_hid_report` is `true`, steering defaults safely to `calibration.center` (`0.0°`) and pedals default cleanly to physical rest (`l_max` = `255` for inverted pedals, `l_min` = `0` for non-inverted pedals -> **`Físico: 0%`**, **`Saída: 0%`**). Re-acquiring `p_cooked_device` during `apply_live_calibration` (`32768 -> 32767`) never falsely clears the guard, and the moment any real HID packet updates any axis, `awaiting_initial_hid_report` clears permanently to live tracking.
  - Added unit test `test_initial_hid_report_guard_suppresses_50_and_91_pct_startup_artifacts`.

## Hardware & Regression Verification
- **Live G920 `speed-precise-cli monitor --device 0` (untouched hardware):**
  - Before fix: `Wheel = 32767 (50.0%)`, `Clutch = 127 (50.0%)`, `Brake = 127 (50.0% -> 91% in GUI)`, `Accelerator = 127 (50.0%)`.
  - After fix: `Wheel = 32768 (50.0% / 0.0°)`, `Clutch = 255 (100.0% -> 0% in GUI)`, `Brake = 255 (100.0% -> 0% in GUI)`, `Accelerator = 255 (100.0% -> 0% in GUI)`.
- **`python run_regression_tests.py`:** **11/11 stages PASSED**, `0` warnings under `-C target-feature=+crt-static -D warnings`, `0` missing `Ubuntu-Light.ttf` glyphs, **47/47 unit tests passed** (`4` CLI + `34` core + `9` GUI), live `speed-precise-cli list` verified attached Logitech G HUB G920 (`046D:C262`).
- **`python build_portable_release.py` (synced across root & `dist/`):**
  - `SpeedPrecise.exe`: `4,113,408 bytes` (`3.92 MiB`), SHA-256 `d1fd1ee7e56659722fc2fa1deaa41d31054b5354e3d4cd050d3fc11c26fcc705`
  - `SpeedPrecise-v1.0.0-win64-portable.zip`: `2,464,761 bytes` (`2.35 MiB`), SHA-256 `bcfd115d8aa3685ea081b1b6ce8a40f468017b651fc10965da2d8f90c79c6490`
