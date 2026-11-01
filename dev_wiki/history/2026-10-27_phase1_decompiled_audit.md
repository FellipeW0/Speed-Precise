# Task Log: Phase 1.1 — Static Audit of Decompiled Apps & Dev-Wiki Bootstrap

- **Date:** 2026-10-27
- **Phase:** 1.1 (Read-Only Reverse Engineering Audit & Governance Bootstrap)

## 1. What Was Done
- Inspected workspace structure (`d:\Speed_Precise_app`), `decompiled _apps/` (`DIView.exe.c`, `WheelCheck.exe.c`, `LUTGenerator.exe.c`), and `Documentation/` (`Documentação_Fase_1.md`, `Documentação_Fase_2.md`, `Documentação_Fase_3.md`, `Skill - Governance and Dev-Wiki.md`).
- Audited `LUTGenerator.exe.c` and disassembled the CIL bytecode and CLI metadata tables (`#Strings`, `TypeRef`, `MemberRef`) of `LUTGenerator.exe` (`LUTFinder.MainWindow` and `LUTGenerator.SubWindow`).
- Audited `WheelCheck.exe.c` (`FUN_00401210`, `FUN_004024d0`, `FUN_00403180`, `FUN_004034a0`, `FUN_00404040`, `FUN_00404230`, `FUN_004078c0`) and extracted default `.data` globals and FPU normalization instructions from `WheelCheck.exe`.
- Audited `DIView.exe.c` (`FUN_004010b0`, `FUN_00401360`, `FUN_004013d0`, `FUN_004015f0`, `FUN_004043d0`, `FUN_00404560`, `FUN_004054f0`, `FUN_0040b490`, `FUN_0040b680`–`FUN_0040b800`) and disassembled the unsymbolized MFC message-map handlers and callbacks (`0x00401EF0`, `0x004020E0`, `0x00402250`, `0x004041F0`, `0x00404C20`, `0x00404DC0`, `0x0040B440`–`0x0040B67F`) from `DIView.exe`.
- Created:
  - `docs/FASE_1_DESCOBERTAS_REAIS.md`
  - `AGENTS.md`
  - `dev_wiki/INDEX.md`
  - `dev_wiki/KNOWN_TRAPS.md`
  - `dev_wiki/history/2026-10-27_phase1_decompiled_audit.md`

## 2. What Failed / Pitfalls Encountered During Inspection
- `docs/FASE_1_ENGENHARIA_REVERSA_E_ESPECIFICACAO.md` did not exist initially because the theoretical specification was located at `Documentation/Documentação_Fase_1.md`, and `decompiled_apps/` was named `decompiled _apps/` (with a space).
- `LUTGenerator.exe.c` contained only PE/CLI metadata structs and `_CorExeMain()` because Ghidra exported it in native x86 mode; resolved by disassembling the CIL bytecode and CLI metadata tables directly from `LUTGenerator.exe` in read-only mode.
- `DIView.exe.c` omitted several MFC message-map handlers (`OnCalibrate`, `OnDeadzone`, `OnSaturation`) and `EnumObjectsCallback` (`0x00404DC0`); resolved by inspecting the exact `.text` machine code ranges in `DIView.exe`.

## 3. What Worked & Key Technical Findings
- Verified exact `dinput8.dll` property IDs (`DIPROP_CALIBRATIONMODE = 10 / 0x0A`, `DIPROP_CALIBRATION = 11 / 0x0B`) and struct sizes (`sizeof(DIPROPHEADER) = 16`, `sizeof(DIPROPCAL) = 28`), correcting critical errors in the theoretical specification.
- Verified that `DIView.exe` persists calibration via `IDirectInputDevice8::SetProperty(DIPROP_CALIBRATION, &dipropcal)` without linking `advapi32.dll`.
- Verified all 6 states (`case 0`–`case 5`) of WheelCheck's `Step Log 2` state machine (`FUN_004078c0`, `DAT_0045bb8c == 0xc`), including the PID centering loop (`FUN_00404230`/`FUN_00404040`), `300 ms` settle time (`0x12a`), `300 ms` step time, integer `startX`/`endX`/`deltaX` formatting (`%d, %d, %d, %d, %f\n`), and unquantized `deltaXDeg` calculation.
