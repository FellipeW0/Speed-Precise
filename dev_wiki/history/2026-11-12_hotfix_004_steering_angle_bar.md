# 2026-11-12 — Hotfix 004: Live Steering Angle Bar Synchronization & Raw Fallback

## Summary
Resolved a visual telemetry bug reported on the main Cockpit screen (`Volante e Pedais`) where the horizontal steering angle gauge (`paint_horizontal_steering_gauge`) and live angle readout (`angle_deg` / `signed_pct`) remained frozen near `0.0°` (`50.0%` center reset) while pedal bars continued updating properly.

## Root Causes
1. **Unilateral GUI Raw-Fallback Gap (`crates/speed-precise-gui/src/app.rs`):** Hotfix 003 introduced `compute_live_pedal_display_percentages` (`evaluate_calibrated_output_pct` from `snap.raw_value`) for the 3 pedal cards (`render_vertical_pedal_card`), whereas `render_horizontal_steering_card` still read `snap.cooked_percent` passively (`unwrap_or(50.0)`), ignoring `snap.raw_value` when `p_cooked_device` returned `dinput8.dll`'s synthetic `32767` (`50.0%`) center-reset value.
2. **Whole-Device `Unacquire`/`Acquire` Cooked Reset (`crates/speed-precise-core/src/device.rs`):** Applying calibration to any single axis (e.g., Brake) in `DualDeviceHandle::apply_live_calibration` calls `Unacquire()` -> `SetProperty()` -> `Acquire()` on `p_cooked_device`, resetting `dinput8.dll`'s cooked state buffer for **all** axes on the device (including axis `0`, `Wheel axis`, `dwType = 0x01000002`), yet `cooked_awaiting_hid_sync` was previously only flagged for the single edited axis index. Additionally, `DualDeviceHandle::open` did not initialize `DIPROP_RANGE` / `DIPROP_CALIBRATIONMODE(0)` + initial `Poll()` on `p_cooked_device`, and `poll_telemetry` lacked symmetric raw/cooked cross-fallback (`resolve_live_axis_telemetry`).
3. **Minor Gauge Render Clip & Fill Contrast (`paint_horizontal_steering_gauge`):** `bar_rect` began directly at `outer_rect.min.y` inside `painter_at(outer_rect)`, clipping the top `1 px` of the `3 px` white needle (`bar_rect.top() - 1.0_f32`), and filled bars used `.gamma_multiply(0.82_f32)`.

## Changes Applied
- **`crates/speed-precise-core/src/device.rs`:**
  - Extended `CockpitAxisMapping::detect` Pass 1 to also match `"x axis"` / `"eixo x"` names cleanly alongside `"wheel"`.
  - Updated `DualDeviceHandle::open` to explicitly apply `set_axis_range(p_cooked_device, axis.dw_type, axis.output_range.0, axis.output_range.1)` + `set_calibration_mode(p_cooked_device, ..., 0)` and perform an initial `Poll()` on both handles after `Acquire()`.
  - Updated `DualDeviceHandle::apply_live_calibration` to flag `cooked_awaiting_hid_sync[i] = true` across all axes whenever `p_cooked_device` undergoes `Unacquire()`/`Acquire()`.
  - Added `pub fn resolve_live_axis_telemetry(...) -> (i32, i32, f64)` to keep `raw_value`, `cooked_value`, and `cooked_percent` synchronized across both `p_raw_device` and `p_cooked_device` even when one handle rests on `dinput8.dll`'s midpoint reset.
  - Cleaned temporary diagnostic code out of `test_directinput8_context_init_and_enumerate` and added `test_resolve_live_axis_telemetry_steering_fallback`.
- **`crates/speed-precise-gui/src/app.rs`:**
  - Added `pub fn compute_live_steering_angle_and_pct(snap: Option<&AxisTelemetrySnapshot>, wheel_rotation_deg: f64) -> (f64, f64)` and wired `render_horizontal_steering_card` to use it.
  - Refined `paint_horizontal_steering_gauge` (`50.0_f32` outer height, `bar_rect` inset `+2.0_f32..+30.0_f32` to prevent needle clipping, solid opaque `COLOR_STEERING_FILL` fill).
  - Added unit test `test_steering_gauge_updates_live_when_cooked_stuck_at_50pct` (`540°`, `900°`, `1080°` presets + cooked-driven + `None` cases).

## Verification & Packaged Release Output
- `python run_regression_tests.py`: **11/11 stages PASSED** (`0` compiler warnings under `-C target-feature=+crt-static -D warnings`, `0` missing `Ubuntu-Light.ttf` glyphs across 15 non-ASCII codepoints):
  - `speed-precise-cli`: `4/4` unit tests passed
  - `speed-precise-core`: `33/33` unit tests passed
  - `speed-precise-gui`: `9/9` unit tests passed
  - **Total unit tests:** `46/46` passed
- `python build_portable_release.py`: **PASSED** (`PE Subsystem == 2`, zero VC++ runtime DLL dependencies)
  - `dist/SpeedPrecise/SpeedPrecise.exe` (and synced copies `SpeedPrecise.exe` and `dist/SpeedPrecise.exe`): `4,110,848 bytes` (`3.92 MiB`), SHA-256 `3e855c76f08025e4a71eb3658c6bcbc0cc781c72c1d0b1d6c0cfdac643c7414b`
  - `dist/SpeedPrecise-v1.0.0-win64-portable.zip`: `2,458,576 bytes` (`2.34 MiB`), SHA-256 `d3004eb11b2e0d68b5113b53bdcc3ae0b8e2e89dd671ed5bc84f156b84fc0f87`
