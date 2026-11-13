# Speed Precise v1.0.1 — Full Application Review, Patch Notes & Release (`Hotfix 004 + Hotfix 005`)

- **Date:** 2026-11-12
- **Release Tag:** `v1.0.1` (SemVer `1.0.1`)
- **Scope:** Full workspace audit, merge with `origin/main` (`a3f7ef7`), version bump (`1.0.0 -> 1.0.1`), `CHANGELOG.md` & `README.md` updates, dynamic versioning in `build_portable_release.py`, `11/11` regression verification, portable `.zip` packaging, and Git push (`main` + `v1.0.1`).

## Summary of Changes
1. **Workspace & Version Synchronization (`1.0.1` SemVer):**
   - Merged `origin/main` commit `a3f7ef7` (`Refactor README for clarity and conciseness`) cleanly via fast-forward.
   - Bumped `[workspace.package].version` in `Cargo.toml` from `1.0.0` to `1.0.1` and aligned `[workspace.package].repository` to `https://github.com/FellipeW0/Speed-Precise`.
   - Updated `.cargo/config.toml` header comment, `.gitignore` (added `/dev_apps/` so legacy reference binaries remain ignored locally), `README.md` (updated download archive names, SHA-256 verification commands, and direct `https://github.com/FellipeW0/Speed-Precise/releases` URL while preserving `a3f7ef7` overview phrasing), and `CHANGELOG.md` (`## [1.0.1] - 2026-11-12` patch notes documenting Hotfix 004 and Hotfix 005).
   - Refactored `build_portable_release.py` to read `VERSION` dynamically from `Cargo.toml` (`read_workspace_version`) and interpolate it into `LEIA-ME.txt` and output filenames.

2. **Included Bug Fixes (`v1.0.1`):**
   - **Hotfix 004:** Resolved live horizontal steering angle gauge freeze caused by `pCookedDevice` remaining stagnant at midpoint (`32768` / `50%`) or awaiting post-`Acquire()` HID sync; unified dual-handle steering/pedal telemetry (`resolve_live_axis_telemetry`, `compute_live_steering_angle_and_pct`) and removed 1 px top needle clip in `paint_horizontal_steering_gauge`.
   - **Hotfix 005:** Eliminated phantom `50%` (Clutch/Throttle) and `91%` (Brake) startup readings caused by `dinput8.dll` returning idle midpoint defaults (`[32767, 127, 127, 127]` raw / `32768` cooked) prior to the first USB HID report arrival (`all_axes_at_dinput_midpoint`, `detect_initial_hid_report_arrival`, `axis_uninitialized_resting_raw`, `resolve_axis_telemetry_with_initial_guard`).

## Verification & Release Artifacts
- **`python run_regression_tests.py`:** `PASS` (`11/11` stages passed, `47/47` unit tests across `speed-precise-cli` [4], `speed-precise-core` [34], `speed-precise-gui` [9], zero warnings under `-C target-feature=+crt-static -D warnings`, `PE Subsystem == 2` static-CRT import check, `Ubuntu-Light.ttf` `cmap` glyph audit `0` missing glyphs).
- **`python build_portable_release.py`:** `PASS`
  - `dist/SpeedPrecise/SpeedPrecise.exe`: `4,113,408` bytes (`3.92 MiB`), SHA-256 `525b399dc3d0c0de2cf121c6bc96ddc5ce672abd5b6ff102deb73e2e29064063` (synced identically to `SpeedPrecise.exe` and `dist/SpeedPrecise.exe`).
  - `dist/SpeedPrecise/speed-precise-cli.exe`: `817,152` bytes (`0.78 MiB`), `speed-precise-cli 1.0.1`.
  - `dist/SpeedPrecise-v1.0.1-win64-portable.zip`: `2,465,570` bytes (`2.35 MiB`), SHA-256 `23ab4667eb2d37dbc133aea3a9e220b484280c0c84b01cb7a5ce387eb99fdf8a`.
