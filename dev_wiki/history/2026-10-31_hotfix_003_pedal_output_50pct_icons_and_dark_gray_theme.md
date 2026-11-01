# Hotfix & UX Polish 003 — Pedal Output 50% Lock Fix, Full Vector Icon Overhaul & Neutral Black/Dark-Gray Theme (2026-10-31)

## 1. Scope & Objectives
1. **Fix Bug 1 (Pedal Output Column Stuck at `50%` When Changing Saturation or Deadzone):**
   - Resolve the DirectInput8 / UI bug on the `Volante e Pedais` screen where altering `Curso Máximo (Saturação)` or `Zona Morta Inicial` froze the wide calibrated Output bar at `50%` instead of tracking real pedal input across `0%..100%`.
2. **Fix Bug 2 (Broken Icon Next to `"Ajuste salvo em 'Brake'..."` & Full Application Icon/Symbol Audit):**
   - Audit every non-ASCII symbol and icon across `crates/speed-precise-gui/src/*.rs` against `egui 0.29`'s bundled TrueType `cmap` tables (`Ubuntu-Light.ttf` + `NotoEmoji-Regular.ttf`) and replace all missing/misaligned Unicode symbols with crisp `egui::Painter` vector icons.
3. **Dark Theme Reformulation (Neutral Black & Dark-Gray Palette):**
   - Replace the predominantly bluish dark palette with a comfortable, high-legibility neutral black & dark-gray (`R == G == B`) palette across all panels, cards, tracks, borders, texts, and interactive controls without altering existing functionality or cockpit visual identity.

## 2. Forensic Root-Cause Findings
1. **Why the Pedal Output Column Froze at `50%` (`TRAP-020`):**
   - Empirical hardware verification on the connected **Logitech G920 (`046D:C262`)** proved that whenever `IDirectInputDevice8W::SetProperty` (`DIPROP_CALIBRATION = 0x0B`) is invoked on `p_cooked_device`, `dinput8.dll` immediately resets `p_cooked_device`'s internal cooked axis state buffer in `GetDeviceState` to **`32767` (`32767 / 65535 = 50.0%`)**, keeping it at `32767` until a new USB HID interrupt packet arrives from physical pedal movement (whereas `p_raw_device` continues reporting the true physical sensor value `255 = 0%`).
   - Additionally, `DualDeviceHandle::apply_live_calibration` previously mutated `0x0B`, `DIPROP_DEADZONE`, and `DIPROP_SATURATION` back-to-back while `p_cooked_device` remained acquired (unlike `DIView.exe` `OnCalibrate @ 0x00402180`, which only writes `0x0B`), and `render_vertical_pedal_card` read `snap.cooked_percent` passively without evaluating DirectInput8's `[lMin, lCenter, lMax]` transfer curve against `snap.raw_value` during slider drags or post-`SetProperty` `50%` resets.
2. **Why `"●"` (`U+25CF`) and Other Icons Rendered Broken (`TRAP-021`):**
   - Binary `cmap` inspection of `epaint_default_fonts` (`Ubuntu-Light.ttf` and `NotoEmoji-Regular.ttf`) revealed:
     - `U+25CF` (`●` in the bottom status bar next to `"Ajuste salvo em 'Brake'..."`), `U+2713` (`✓` in `"Aplicar nos Pedais"`), and `U+27F3` (`⟳` in `"Atualizar USB"`) are **absent from BOTH fonts** (`Ubuntu=False, NotoEmoji=False`), rendering as broken missing-glyph tofu boxes (`□`).
     - `U+25B6` (`▶`), `U+26A0` (`⚠`), `U+26D4` (`⛔`), `U+1F4BE` (`💾`), `U+1F4C2` (`📂`), `U+1F50D` (`🔍`), and `U+1F680` (`🚀`) are absent from `Ubuntu-Light.ttf` and fall back to monochrome `NotoEmoji-Regular.ttf` with mismatched baseline/ascent metrics.

## 3. Changes Implemented
- **`crates/speed-precise-core/src/calibration.rs` & `lib.rs`:**
  - Implemented and exported `evaluate_calibrated_output_pct(raw_value, cal, deadzone, saturation) -> f64`, reproducing DirectInput8's exact 3-point `[lMin, lCenter, lMax]` piecewise-linear transfer curve (`0.0..=100.0`), verified by `test_evaluate_calibrated_output_pct_inverted_g920_and_non_inverted`.
- **`crates/speed-precise-core/src/device.rs`:**
  - Updated `DualDeviceHandle::apply_live_calibration` to `Unacquire()` before `SetProperty(0x0B)`, skip redundant `DIPROP_DEADZONE`/`DIPROP_SATURATION` writes when already matching, re-`Acquire()` + `Poll()`, and flag `cooked_awaiting_hid_sync[pos] = true`.
  - Updated `DualDeviceHandle::poll_telemetry` to evaluate `cooked_percent` and `cooked_value` directly from `raw_value` + `axis.calibration` whenever `p_cooked_device` sits on `dinput8.dll`'s post-`SetProperty` synthetic `32767` (`50.0%`) buffer reset awaiting a new HID report.
- **`crates/speed-precise-gui/src/app.rs`:**
  - Added `compute_live_pedal_display_percentages` and wired it into `render_vertical_pedal_card`, providing instant real-time Output column updates while dragging Saturation/Deadzone sliders and eliminating any `50%` freeze.
  - Replaced all missing/fallback Unicode icon characters with crisp `egui::Painter` vector primitives (`paint_status_led`, `paint_warning_icon`, `VectorIcon::{Refresh, Check, Play, Folder, Stop, ResetZoom, ApplyCheck, Save}`, and `add_icon_button`).
  - Reformulated the dark theme (`COLOR_BG_*`, `COLOR_BORDER_SUBTLE`, `COLOR_TEXT_*`, `COLOR_ACCENT_*`, and `configure_cockpit_dark_theme`) to a comfortable, neutral black & dark-gray (`R == G == B`) hierarchy (`#101010` tracks, `#141414` root, `#1A1A1A` panels, `#222222` cards, `#303030..#525252` controls, `#F2F2F2` / `#D4D4D4` / `#A3A3A3` high-contrast text).
  - Added unit tests `test_pedal_output_updates_across_saturation_and_deadzone_without_50pct_lock` and `test_dark_theme_palette_is_neutral_gray_without_blue_cast`.
- **`run_regression_tests.py`:**
  - Added Stage 11 (`verify_gui_font_glyph_coverage`) auditing all non-comment codepoints in `crates/speed-precise-gui/src/*.rs` directly against `Ubuntu-Light.ttf`'s TrueType `cmap` table.

## 4. Verification Results
- `python run_regression_tests.py`: **11/11 stages PASSED** (`0` warnings under `RUSTFLAGS="-D warnings"`).
- `cargo test --workspace`: **44/44 unit tests PASSED** (`speed-precise-core`: `32`, `speed-precise-cli`: `4`, `speed-precise-gui`: `8`).
- Native `debug` and `release` `speed-precise-gui.exe` binaries rebuilt and verified (`0` missing font glyphs).
