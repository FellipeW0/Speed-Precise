# Task Report — Hotfix & Regression 001: Logitech G920 End-Stop Slamming Bug in PID & Step Log 2

- **Date:** 2026-10-27
- **Phase:** Hotfix & Regression 001 (Phase 2 Hardware Validation)
- **Status:** Completed & Verified (`python run_regression_tests.py` 7/7 checks passing; 27/27 unit tests passing, 0 compiler warnings)

## 1. Hardware Bug Symptom (Logitech G920 `046D:C262`, `LogicalRange = [0, 65535]`)
When executing `step-log2` on a physical Logitech G920 (`[0, 65535]`, where `0` is $-450^\circ$ full left lock, `32768` is $0^\circ$ physical center, and `65535` is $+450^\circ$ full right lock), instead of starting at `32768` ($0^\circ$), applying a step pulse, and smoothly returning to center, the wheel violently rotated into the mechanical end-stop, backed off slightly during the `300 ms` motor-off settle window, and slammed into the end-stop repeatedly.

## 2. Root Causes Verified in `decompiled _apps/WheelCheck.exe.c`
1. **Inverted PID Error Sign (`target_x - pos_x` vs `pos_x - target_x` in `FUN_00404040` line `3108`):**
   - In DirectInput8 `GUID_ConstantForce` (`DIEFF_CARTESIAN` with `rglDirection[0] = 1`), a **positive** `lMagnitude > 0` pulls the steering wheel **left** (decreasing `rawX` toward `lMin = 0` / `-10000`), and a **negative** `lMagnitude < 0` pulls the steering wheel **right** (increasing `rawX` toward `lMax = 65535` / `+10000`).
   - Consequently, `WheelCheck.exe` (`FUN_00404040`, line `3108`) computes `iVar2 = param_3 - param_2` (`err = posX - targetX`, NOT `targetX - posX`).
   - Our previous `PidCenteringController::update` computed `err = target_x_norm - pos_x_norm`, which inverted the feedback loop into **positive feedback** on the real G920, driving the motor at `100%` torque (`-10000`) straight into the right mechanical end-stop (`65535`) until the `10000 ms` timeout!
2. **Inverted Pulse Polarity in State 4 (`FUN_004078c0` lines `4965` & `5048`):**
   - At line `4965`, `DAT_0045be38 = (-(uint)(DAT_00458006 != '\0') & 2) - 1` evaluates to **`+1`** when `DAT_00458006 == 1` (`0xFFFFFFFF & 2 == 2`, `2 - 1 == +1`).
   - Thus, in State 4 (line `5048`), `local_148` is **positive** (`+step_csv_force`), rotating the wheel toward `lMin` (`rawEndX < rawStartX`) so `-(rawEndX - rawStartX) > 0` and `-deltaX > 0`.
   - Furthermore, `VirtualWheelSimulator` previously had inverted force physics (`+force` increased `rawX`), which cancelled out the inverted PID sign during `--dry-run` tests.
3. **Degenerate Cartesian Direction Vector (`rglDirection[0] = 0` instead of `1`) & `use_object_ids` Flag in `FfbSession`:**
   - `WheelCheck.exe` (`FUN_004034a0` line `2826` and `FUN_004078c0` line `5903`) initializes `rglDirection[0] = 1` for `DIEFF_CARTESIAN`. Our `FfbSession::open` previously initialized `dir_box = 0` and rebuilt `DiEffect` with `use_object_ids = false` in `apply_force` even when `CreateEffect` succeeded with `use_object_ids = true`.
4. **Unclamped PID Centering Torque (`Kp = 5.0` saturating at `10000` at $90^\circ$):**
   - Capping PID centering torque to `DEFAULT_MAX_CENTERING_FORCE = 2800` (`28%` max torque) with a `120 ms` soft-start ramp and a `±12` count center deadband ensures gentle, overdamped centering without violent snapping.

## 3. Fixes & Safety Enhancements Applied
- **`crates/speed-precise-core/src/ffb_engine.rs`:**
  - Added `logical_center_raw(0, 65535) == 32768` and `normalize_raw_to_unit(32768, 0, 65535) == 0.0`.
  - Fixed `PidCenteringController::update` to compute `err = pos_x_norm - target_x_norm`, added `max_centering_force = 2800` (`28%` max torque cap), a `120 ms` soft-start envelope, and a center deadband around `32768` (`0`).
  - Fixed `FfbSession` to set `*dir_box = 1`, preserve `self.use_object_ids` in `apply_force` and `emergency_stop`, set `DIPROP_RANGE` via both `DIPH_BYID` and `DIPH_DEVICE`, and read back the active `DIPROP_RANGE`.
  - Updated `ENDSTOP_SAFETY_LIMIT_NORM` to `5_000` (`50%` physical travel = $\pm 225^\circ$, raw `[16384, 49152]`) and added consecutive runaway detection in State 2 (`PidCenter`) that zeroes motor force and aborts immediately if the wheel moves away from center.
  - Aligned `VirtualWheelSimulator` physics polarity with real DirectInput8 hardware (`+force` moves toward `lMin = 0`, `-force` moves toward `lMax = 65535`).
  - Added `test_reg_001_g920_unsigned_logical_range_centering_and_no_endstop_slam` and `test_reg_001_pid_runaway_safety_cutoff_if_polarity_inverted`.
- **`crates/speed-precise-cli/src/main.rs`:**
  - Added `--max-force <i32>` and `--direction <i32>` to `step-log2` and `auto-tune` subcommands for gentle hardware validation (e.g., `--max-force 3500 --steps 10`).
