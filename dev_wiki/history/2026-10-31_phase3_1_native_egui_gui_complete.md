# Session Log — 2026-10-31: Phase 2.3 & Phase 3.1 Native Desktop GUI (`eframe` + `egui` + `egui_plot`)

## Summary
Completed the native Rust graphical desktop studio (`crates/speed-precise-gui`) built with `eframe 0.29`, `egui 0.29`, `egui_plot 0.29`, and `rfd 0.15`, integrating all three core modules (`DIView` live Raw/Cooked calibration, `WheelCheck` `1000 Hz` `Step Log 2` Force Feedback testing, and real-time `< 1 ms` `LUTGenerator` + 1-click Assetto Corsa / BeamNG profile injection) behind a strictly decoupled multi-threaded hardware worker (`worker.rs`).

## Architectural Changes
1. **`speed-precise-core` (`lut_math.rs`, `sim_inject.rs`, `lib.rs`):**
   - Verified `lut_math.rs` (`parse_wheelcheck_csv`, `read_wheelcheck_csv_file`, `generate_inverse_lut`, `write_lut_file`) and `sim_inject.rs` (`inject_assetto_corsa_lut`, `update_ff_post_process_ini`).
   - Added `BeamNgInjectionOutcome`, `default_beamng_inputmaps_dir`, and `inject_beamng_lut` (plus unit test `test_inject_beamng_lut_writes_valid_lut_file`) in `sim_inject.rs` and re-exported them in `lib.rs`.
2. **Multi-Threaded Hardware Isolation (`crates/speed-precise-gui/src/worker.rs`):**
   - Implemented `HardwareWorker`, `WorkerConfig`, `GuiCommand`, and `WorkerEvent` communicating over `std::sync::mpsc` + `egui::Context::request_repaint()`.
   - Paired `GuiCommand::EmergencyStopFfb` with a shared `Arc<AtomicBool>` (`ffb_abort_flag`) checked on every `1000 Hz` (`1 ms`) tick of `run_step_log2_with_backend` for `< 1 ms` emergency motor torque cutoff (`apply_force(0)` + RAII `FfbSession::drop` -> `Stop()` + `Unacquire()`).
   - Automated clean handle handoff: releases non-exclusive `DualDeviceHandle` (`DISCL_NONEXCLUSIVE | DISCL_BACKGROUND`) before acquiring `FfbSession` (`DISCL_EXCLUSIVE`) and reopens `DualDeviceHandle` automatically upon test completion or abort.
   - Added zero-overhead `GuiMonitoredBackend` decorator around `FfbHardwareBackend` to detect live intra-step phase transitions (`Breakaway Wake-Up` -> `PID Centering` -> `Settle 300ms` -> `Force Pulse`).
3. **Unified 3-Tab Sim-Racing Interface (`crates/speed-precise-gui/src/app.rs` & `main.rs`):**
   - **Persistent Top Header Bar:** USB controller `ComboBox` (`VID:PID` + Name + `[FFB]` badge), `⟳ Refresh USB`, and an always-visible bright-red `⛔ PARADA DE EMERGÊNCIA FFB (ESPAÇO / ESC)` button paired with global `Key::Space` and `Key::Escape` hotkeys.
   - **Tab 1 (`DIView` Module):** Per-axis live dual horizontal progress bars (`Sinal Bruto (Raw)` vs. `Sinal Calibrado (DirectInput)`), numeric inputs, `[Capturar Min Atual]`, `[Capturar Centro Atual]`, `[Capturar Max Atual]`, `Deadzone (0..10000)` and `Saturation (0..10000)` sliders, `[Aplicar ao Vivo no Registro (0x0B)]` (with automatic JSON backup in `%APPDATA%\SpeedPrecise\backups\`), and `[Restaurar Padrão]`.
   - **Tab 2 (`WheelCheck Step Log 2` Module):** Interactive parameters (`Passos 10..100`, `Duração 300 ms`, `Ângulo 900.0°`, `Força Máxima 1000..10000`, `Modo Simulador Virtual --dry-run`), `[Iniciar Ensaio Step Log 2]`, `[Importar CSV Existente]`, `[Salvar CSV]`, live progress bar + state badge, and live `egui_plot` `(force, deltaXDeg)` curve.
   - **Tab 3 (`LUT Generator` & Simulator Injection):** Automatic synchronization from Tab 2 or imported `.csv`, `< 1 ms` real-time sliders (`Janela de Suavização 1..15`, `Passos da LUT 50 / 100`), diagnostic metric cards (**Zona Morta Mecânica Detectada (%)**, **Ângulo Máximo (`maxDeltaXDeg`)**, **Picos Corrigidos**), 3-curve `egui_plot` (`Linear Reference 45°`, `Raw Wheel Response`, `Corrected Inverse LUT`), `[Salvar Arquivo .LUT]`, `[Injetar Automaticamente no Assetto Corsa (ff_post_process.ini)]`, and `[Exportar BeamNG (wheel.lut)]`.
4. **Master Regression Test Suite (`run_regression_tests.py`):**
   - Expanded to 9 deterministic checks enforcing `-D warnings` across all 3 workspace crates (`speed-precise-core`, `speed-precise-cli`, `speed-precise-gui`), running all 37 unit tests, verifying GUI binary linkage, and validating CLI `auto-tune --dry-run --inject-ac` CSV/LUT/INI artifacts.

## Verification
- `python run_regression_tests.py`: **PASS** (`9/9` checks passed, `37/37` unit tests passed across `core` [28], `cli` [4], and `gui` [5], `0` compiler warnings).
- `cargo build --release -p speed-precise-gui`: **PASS** (optimized LTO native Win32 GUI binary).
