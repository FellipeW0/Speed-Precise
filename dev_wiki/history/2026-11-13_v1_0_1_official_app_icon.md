# Speed Precise v1.0.1 — Official Application Icon Incorporation (`Speed_Precise.ico`)

- **Date:** 2026-11-13
- **Release Tag:** `v1.0.1` (updated with official `Speed_Precise.ico`)
- **Scope:** Embedded `Speed_Precise.ico` (`256×256` PNG-encoded Windows ICO) into the Windows PE executable `.rsrc` (`RT_ICON` / `RT_GROUP_ICON` + `VS_VERSION_INFO`), OS window/taskbar icon (`ViewportBuilder::with_icon`), and top cockpit header badge.

## Summary of Changes
1. **Windows PE Resource & Version Metadata (`crates/speed-precise-gui/build.rs`):**
   - Added deterministic build script locating Windows SDK `rc.exe` (`PATH` / `RC` env / Windows Kits `10`/`11` registry & SDK folders).
   - Compiles resource ID `1 ICON` pointing to root `Speed_Precise.ico` plus `1 VERSIONINFO` (`1.0.1.0`, `Speed Precise`, `SpeedPrecise.exe`) into `%OUT_DIR%/app_icon.res` and links via `cargo:rustc-link-arg-bins`.
2. **Runtime Icon & Header Brand Badge (`crates/speed-precise-gui/src/app_icon.rs`, `main.rs`, `app.rs`):**
   - `extract_largest_png_from_ico` parses Windows `ICONDIR` / `ICONDIRENTRY` headers and feeds the embedded `256×256` PNG frame to `eframe::icon_data::from_png_bytes`.
   - `main.rs` attaches the decoded `IconData` to `egui::ViewportBuilder::with_icon(...)` for taskbar, Alt+Tab, and titlebar icons.
   - `app.rs` renders a clean `32×32` logo inside a compact `#ECECEC` rounded pill beside `"SPEED PRECISE"` in `render_top_header`.
3. **Automated PE Resource Guard & Documentation:**
   - Extended `run_regression_tests.py` (`_inspect_pe_subsystem_and_imports`) to assert `RT_ICON` (`3`), `RT_GROUP_ICON` (`14`), and `RT_VERSION` (`16`) in `target/release/speed-precise-gui.exe`.
   - Added icon incorporation note under `### Adicionado` in `CHANGELOG.md` (`[1.0.1]`).

## Verification & Release Artifacts
- **`python run_regression_tests.py`:** `PASS` (`11/11` stages, `48/48` unit tests including `test_official_app_ico_decodes_cleanly_to_256px_rgba`, PE resource IDs `[3, 14, 16]` verified, `0` warnings under `+crt-static -D warnings`, `0` missing font glyphs).
- **Windows Shell Extraction Verification:** `[System.Drawing.Icon]::ExtractAssociatedIcon` and `FileVersionInfo::GetVersionInfo` verified `32x32` Shell icon and `Speed Precise` `1.0.1.0` metadata directly on `SpeedPrecise.exe`.
- **`python build_portable_release.py`:**
  - `SpeedPrecise.exe` (`root`, `dist/`, and `dist/SpeedPrecise/`): `4,171,264` bytes (`3.98 MiB`), SHA-256 `F39EEB08F1D31FD6E61D7A6A786969FD396A06211D9731937A982E28DDFEA18F`.
  - `dist/SpeedPrecise-v1.0.1-win64-portable.zip`: `2,522,381` bytes (`2.41 MiB`), SHA-256 `76B2B3C82740E236C20FEC5AFD28D892AD0B7E2E358448E5A4E93044B96249D7`.
