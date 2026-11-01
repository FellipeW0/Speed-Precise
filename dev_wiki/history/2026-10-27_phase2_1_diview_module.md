# Session Log — 2026-10-27: Phase 2.1 Rust Workspace Bootstrap & DIView Module

## 1. Objective
Initialize the Rust Cargo Workspace (`crates/speed-precise-core` and `crates/speed-precise-cli`), implement the verified DirectInput8 FFI layer (`dinput_ffi.rs`), dual-handle (`pCookedDevice` + `pRawDevice`) device enumeration and polling (`device.rs`), live calibration (`calibration.rs`), CLI subcommands (`list`, `monitor`, `calibrate`), and the master regression runner (`run_regression_tests.py`).

## 2. Files Created / Modified
- `Cargo.toml` — Workspace manifest (`speed-precise-core`, `speed-precise-cli`).
- `crates/speed-precise-core/Cargo.toml` & `src/lib.rs` — Core library configuration and public exports.
- `crates/speed-precise-core/src/error.rs` — Unified `CoreError` (`thiserror`) and `check_hresult`.
- `crates/speed-precise-core/src/dinput_ffi.rs` — Exact `#[repr(C)]` structs (`DipropHeader = 16`, `DipropDword = 20`, `DipropRange = 24`, `DipropCal = 28`, `DipropGuidAndPath = 552`), `MAKEDIPROP` constants (`DIPROP_CALIBRATIONMODE = 10`, `DIPROP_CALIBRATION = 11`, etc.), and `IDirectInput8W` / `IDirectInputDevice8W` COM vtables.
- `crates/speed-precise-core/src/calibration.rs` — `AxisCalibration`, DIView default reset math (`min + (max - min + 1) / 2`), DIView raw normalization (`FUN_00402210`), live `GetProperty`/`SetProperty` wrappers, and `%APPDATA%\SpeedPrecise\backups\` JSON persistence.
- `crates/speed-precise-core/src/device.rs` — `DirectInputContext`, `enumerate_controllers`, and `DualDeviceHandle` (`pCookedDevice` + `pRawDevice` with `DISCL_NONEXCLUSIVE | DISCL_BACKGROUND = 10`).
- `crates/speed-precise-cli/Cargo.toml` & `src/main.rs` — CLI implementation (`list`, `monitor`, `calibrate`).
- `run_regression_tests.py` — Deterministic 4-stage regression runner.
- `AGENTS.md`, `dev_wiki/INDEX.md`, `dev_wiki/KNOWN_TRAPS.md`.

## 3. Verification Results
- `python run_regression_tests.py` passed 4/4 stages with exit code `0`:
  - `cargo check --workspace --all-targets`: 0 errors, 0 warnings.
  - `cargo test --workspace`: 13/13 unit tests passed (11 in `speed-precise-core`, 2 in `speed-precise-cli`).
  - `speed-precise-cli --help` and `speed-precise-cli list`: Verified live `DirectInput8Create` and `EnumDevices` execution.
