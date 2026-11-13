# 2026-11-13 — `v1.0.2`: Auto-LUT Plot/Layout Polish, 200 Steps & Official Icon Release

## Scope & User Requests Covered
- Consolidated the official application icon (`Speed_Precise.ico`, keeping a clean text-only top bar in the UI) with the new `v1.0.2` UI/plot refinements requested in Screen 2 (`Calibração FFB & LUT`):
  1. **LUT Plot Grid Cleanup (`crates/speed-precise-gui/src/app.rs`):** Replaced `log_grid_spacer(10)` with `uniform_grid_spacer` backed by `clean_unit_grid_step_sizes` on `Plot::new("clean_bounded_lut_plot")` to eliminate cluttered minor `0.01` background stripes while preserving clean major divisions (`0.1` / `0.2` / `0.5`).
  2. **LUT Y-Axis Label Non-Overlap (`crates/speed-precise-gui/src/app.rs`):** Configured `.y_axis_min_width(LUT_PLOT_Y_AXIS_MIN_WIDTH)` (`48.0`) so `"Resposta do Volante / Torque Corrigido (0.00 a 1.00)"` no longer overlaps tick labels (`0.0`..`0.9`).
  3. **Full-Width Auto-LUT Control Card (`crates/speed-precise-gui/src/app.rs`):** Extracted `render_import_csv_button`, `render_start_step_log2_button`, `render_autolut_parameter_controls`, and `render_autolut_cta_buttons`; forced full available card width (`ui.set_min_width(avail_w)`) with responsive wide (`>= 960.0`) right-aligned CTAs and narrow fallback.
  4. **Step Count Raised to 200 (`crates/speed-precise-gui/src/app.rs`):** Updated `Passos de Leitura` slider range from `10..=100` to `MIN_STEP_LOG2_UI_STEPS..=MAX_STEP_LOG2_UI_STEPS` (`10..=200`), adding regression test `test_autolut_step_count_200_and_clean_grid_without_dense_sublines`.
  5. **Version & Release Documentation (`Cargo.toml`, `Cargo.lock`, `README.md`, `CHANGELOG.md`):** Bumped workspace version from `1.0.1` to `1.0.2`, documented `Speed_Precise.ico` and Auto-LUT changes under `[1.0.2] - 2026-11-13`.

## Verification Summary
- **`python run_regression_tests.py`:** `PASS` (**11/11 checks passed**, exit code 0).
  - Unit tests: **49/49 passed** (`speed-precise-cli`: 4, `speed-precise-core`: 34, `speed-precise-gui`: 11), `0` failed, `0` ignored.
  - Compiler flags: `+crt-static -D warnings` (zero warnings).
  - PE32+ Audit: Subsystem `2` (`IMAGE_SUBSYSTEM_WINDOWS_GUI`), PE resources `[3, 14, 16]` (`RT_ICON`, `RT_GROUP_ICON`, `RT_VERSION`), system DLLs only.
  - `Ubuntu-Light.ttf` `cmap` audit: `15` distinct non-ASCII codepoints audited across `speed-precise-gui/src/*.rs`, `0` missing glyphs.
- **`python build_portable_release.py` (`v1.0.2`):** `PASS` (exit code 0).
  - `dist\SpeedPrecise\SpeedPrecise.exe` & `dist\SpeedPrecise.exe`: `4,167,680` bytes, SHA-256 `3fd2fca7bd576096af17035ffe4eb08014357a0ae6403570c0ec8c2ade9db0ca`, `DIST_EXE_ICON=32x32`, `FileVersion=1.0.2.0`, `ProductVersion=1.0.2`. (Note: root `SpeedPrecise.exe` remained locked by running PID `16400` during copy).
  - `dist\SpeedPrecise\speed-precise-cli.exe`: `817,152` bytes (`speed-precise-cli 1.0.2`).
  - `dist\SpeedPrecise-v1.0.2-win64-portable.zip`: `2,520,438` bytes, SHA-256 `782f885cf8c883a607da4d03eaa2f925f5d40a9fca8db0eb57e4cefcd37a74db`.
