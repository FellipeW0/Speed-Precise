# AGENTS.md — Global Governance & Engineering Rules for Speed Precise

## 1. Non-Negotiable Operational Lifecycle
Every AI coding agent session MUST follow this 3-step lifecycle on every prompt:
1. **Pre-Action Consultation:** Read `dev_wiki/INDEX.md` and `dev_wiki/KNOWN_TRAPS.md` (plus `docs/FASE_1_DESCOBERTAS_REAIS.md` when touching hardware/DirectInput/LUT logic) before modifying any file.
2. **Scoped Execution & Verification:**
   - Strictly obey the prompt's **Allowed Files** list. Never modify files outside the allowlist.
   - Treat `decompiled _apps/`, `Documentation/`, and legacy `.exe` binaries (`DIView.exe`, `WheelCheck.exe`, `LUTGenerator.exe`) as strictly **READ-ONLY**.
   - Never read entire Ghidra `.c` files at once; always search with `rg` or `Select-String` first and read 100–150 line slices.
   - Run static compilation/syntax checks (`cargo check`, `cargo test`) and the master regression test suite (`python run_regression_tests.py`) before finishing any code mutation task.
3. **Post-Action Logging:**
   - Log a concise task report in `dev_wiki/history/YYYY-MM-DD_<task_slug>.md`.
   - Update `dev_wiki/INDEX.md` (current phase, active task, test status) and `dev_wiki/KNOWN_TRAPS.md` (if new pitfalls or hardware/API quirks were discovered).

## 2. Language & Reporting Rule
- **Mandatory Portuguese (PT-BR) Delivery Reports:** Always write final delivery reports and explanations to the user in **Portuguese (PT-BR)**. Code identifiers, comments, and technical documentation may remain in English or PT-BR as appropriate.

## 3. Hardware & Safety Invariants (`speed-precise-core`)
- **Emergency Motor Cutoff (`FfbSafetyGuard`):** Any struct holding an active `IDirectInputEffect` MUST implement `Drop` to call `Stop()` and `Unacquire()` unconditionally, paired with a `ctrlc` atomic abort handler in CLI mode and `Space`/`ESC` hotkeys in GUI mode.
- **Registry Calibration Backup:** Before writing calibration changes for any `VID:PID`, persist a backup of the previous calibration state to `%APPDATA%\SpeedPrecise\backups\`.
- **DirectInput8 Struct Layout Accuracy:** Always use `sizeof(DIPROPHEADER) == 16` (`0x10`), `sizeof(DIPROPDWORD) == 20` (`0x14`), `sizeof(DIPROPRANGE) == 24` (`0x18`), and `sizeof(DIPROPCAL) == 28` (`0x1C`), with `DIPROP_CALIBRATIONMODE = 10` (`0x0A`) and `DIPROP_CALIBRATION = 11` (`0x0B`).
