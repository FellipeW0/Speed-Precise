# Speed Precise v1.0.1 — Official Application Icon Incorporation (`Speed_Precise.ico`)

- **Date:** 2026-11-13
- **Release Tag:** `v1.0.1` (updated with official `Speed_Precise.ico`)
- **Scope:** Embedded `Speed_Precise.ico` (`256×256` PNG-encoded Windows ICO) into the Windows PE executable `.rsrc` (`RT_ICON` / `RT_GROUP_ICON` + `VS_VERSION_INFO`) and OS window/titlebar/taskbar icon (`ViewportBuilder::with_icon`), keeping the internal app header clean without an inline image badge.

## Summary of Changes
1. **Windows PE Resource & Version Metadata (`crates/speed-precise-gui/build.rs`):**
   - Added deterministic build script locating Windows SDK `rc.exe` (`PATH` / `RC` env / Windows Kits `10`/`11` registry & SDK folders).
   - Compiles resource ID `1 ICON` pointing to root `Speed_Precise.ico` plus `1 VERSIONINFO` (`1.0.1.0`, `Speed Precise`, `SpeedPrecise.exe`) into `%OUT_DIR%/app_icon.res` and links via `cargo:rustc-link-arg-bins`.
2. **Runtime OS Window & Taskbar Icon (`crates/speed-precise-gui/src/app_icon.rs`, `main.rs`):**
   - `extract_largest_png_from_ico` parses Windows `ICONDIR` / `ICONDIRENTRY` headers and feeds the embedded `256×256` PNG frame to `eframe::icon_data::from_png_bytes`.
   - `main.rs` attaches the decoded `IconData` to `egui::ViewportBuilder::with_icon(...)` for taskbar, Alt+Tab, and titlebar icons (while preserving the clean text-only top bar in `app.rs` per user preference).
3. **Automated PE Resource Guard & Documentation:**
   - Extended `run_regression_tests.py` (`_inspect_pe_subsystem_and_imports`) to assert `RT_ICON` (`3`), `RT_GROUP_ICON` (`14`), and `RT_VERSION` (`16`) in `target/release/speed-precise-gui.exe`.
   - Documented under `### Adicionado` in `CHANGELOG.md` (`[1.0.1]`).

## Verification & Release Artifacts
- **`python run_regression_tests.py`:** `PASS` (`11/11` stages, `48/48` unit tests including `test_official_app_ico_decodes_cleanly_to_256px_rgba`, PE resource IDs `[3, 14, 16]` verified, `0` warnings under `+crt-static -D warnings`, `0` missing font glyphs).
- **Windows Shell Extraction Verification:** `[System.Drawing.Icon]::ExtractAssociatedIcon` confirmed `32x32` Shell icon directly on `SpeedPrecise.exe`.
- **`python build_portable_release.py`:**
  - `SpeedPrecise.exe` (`root`, `dist/`, and `dist/SpeedPrecise/`): `4,168,704` bytes (`3.98 MiB`), SHA-256 `49CA997942A8D9F4A0C54F9F90FE32675758FE588762F7A2DA28C18F329C26D5`.
  - `dist/SpeedPrecise-v1.0.1-win64-portable.zip`: `2,520,417` bytes (`2.40 MiB`), SHA-256 `D14AA0210980093C7C5FDF600F9E6D839848691169BAC5EDAAA4E09FD35E6CFA`.
