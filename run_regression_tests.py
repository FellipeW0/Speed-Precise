#!/usr/bin/env python3
"""
Master Deterministic Regression Suite for Speed Precise (Phase 2.3, Phase 3.1 & Phase 3.2).
Executes static compilation checks (across `speed-precise-core`, `speed-precise-cli`,
and `speed-precise-gui`), unit & worker/GUI state tests (including Smart Pedal Saturation math,
Cockpit Axis Auto-Mapping, Unified Auto-LUT, and `[0.0, 1.0]` bounded plot zoom clamping),
native debug & release GUI binary linkage, and CLI end-to-end smoke tests (`list`,
`auto-tune --dry-run --inject-ac`) in isolated subprocesses with strict `-D warnings` enforcement.
"""

import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unicodedata
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parent


def resolve_cargo_env() -> dict[str, str]:
    env = os.environ.copy()
    cargo_bin_dir = Path.home() / ".cargo" / "bin"
    if cargo_bin_dir.is_dir():
        current_path = env.get("PATH", "")
        if str(cargo_bin_dir).lower() not in current_path.lower():
            env["PATH"] = f"{cargo_bin_dir}{os.pathsep}{current_path}"
    # Apply `-D warnings` uniformly across all Cargo invocations (and preserve `.cargo/config.toml`
    # `+crt-static` on Windows MSVC so Cargo shares a single incremental fingerprint cache).
    existing_flags = env.get("RUSTFLAGS", "").strip()
    required_tokens = []
    if os.name == "nt" and "+crt-static" not in existing_flags:
        required_tokens.append("-C target-feature=+crt-static")
    if "-D warnings" not in existing_flags:
        required_tokens.append("-D warnings")
    if required_tokens:
        env["RUSTFLAGS"] = f"{existing_flags} {' '.join(required_tokens)}".strip()
    return env


def resolve_cargo_executable(env: dict[str, str]) -> str:
    found = shutil.which("cargo", path=env.get("PATH"))
    if found:
        return found
    fallback = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if fallback.is_file():
        return str(fallback)
    return "cargo"


def run_step(
    name: str,
    cmd: list[str],
    env: dict[str, str],
    timeout_sec: int = 240,
) -> bool:
    print(f"\n[STEP] {name}")
    print(f"       Command: {' '.join(cmd)}")

    start = time.perf_counter()
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(WORKSPACE_ROOT),
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout_sec,
            encoding="utf-8",
            errors="replace",
        )
    except subprocess.TimeoutExpired:
        print(f"[FAIL] {name} timed out after {timeout_sec}s")
        return False
    except FileNotFoundError as exc:
        print(f"[FAIL] {name} executable not found: {exc}")
        return False

    elapsed = time.perf_counter() - start
    if proc.stdout.strip():
        print(proc.stdout.rstrip())
    if proc.stderr.strip():
        print(proc.stderr.rstrip())

    if proc.returncode != 0:
        print(f"[FAIL] {name} exited with code {proc.returncode} ({elapsed:.2f}s)")
        return False

    print(f"[PASS] {name} completed in {elapsed:.2f}s")
    return True


def _inspect_pe_subsystem_and_imports(exe_path: Path) -> tuple[int, list[str], list[int]]:
    """Parses PE32+ header of `exe_path` to return `(subsystem, imported_dll_names, rsrc_type_ids)`."""
    data = exe_path.read_bytes()
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\x00\x00":
        raise ValueError(f"Invalid PE signature in {exe_path}")

    num_sections = struct.unpack_from("<H", data, pe_off + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe_off + 20)[0]
    opt_off = pe_off + 24
    magic = struct.unpack_from("<H", data, opt_off)[0]
    subsystem = struct.unpack_from("<H", data, opt_off + 68)[0]

    dd_off = opt_off + (112 if magic == 0x20B else 96)
    import_rva, import_size = struct.unpack_from("<II", data, dd_off + 8)
    rsrc_rva, rsrc_size = struct.unpack_from("<II", data, dd_off + 16)

    sec_off = opt_off + opt_size
    sections: list[tuple[int, int, int]] = []
    for i in range(num_sections):
        soff = sec_off + i * 40
        vsize, vrva, raw_size, raw_ptr = struct.unpack_from("<IIII", data, soff + 8)
        sections.append((vrva, max(vsize, raw_size), raw_ptr))

    def rva_to_off(rva: int) -> int | None:
        for vrva, span, raw_ptr in sections:
            if vrva <= rva < vrva + span:
                return raw_ptr + (rva - vrva)
        return None

    dlls: list[str] = []
    if import_rva and import_size:
        imp_off = rva_to_off(import_rva)
        if imp_off is not None:
            while imp_off + 20 <= len(data):
                orig_thunk, _, _, name_rva, first_thunk = struct.unpack_from("<IIIII", data, imp_off)
                if orig_thunk == 0 and name_rva == 0 and first_thunk == 0:
                    break
                noff = rva_to_off(name_rva)
                if noff is not None:
                    end = data.find(b"\x00", noff)
                    if end != -1:
                        dlls.append(data[noff:end].decode("ascii", errors="replace"))
                imp_off += 20

    rsrc_ids: list[int] = []
    if rsrc_rva and rsrc_size:
        rsrc_off = rva_to_off(rsrc_rva)
        if rsrc_off is not None and rsrc_off + 16 <= len(data):
            named_cnt, id_cnt = struct.unpack_from("<HH", data, rsrc_off + 12)
            for idx in range(id_cnt):
                entry_off = rsrc_off + 16 + (named_cnt + idx) * 8
                if entry_off + 8 <= len(data):
                    rsrc_ids.append(struct.unpack_from("<I", data, entry_off)[0])

    return subsystem, dlls, rsrc_ids


def verify_gui_binary_exists() -> bool:
    print("\n[STEP] Verify Native Portable Executables (`debug` & `release`, Windows GUI Subsystem & Static CRT)")
    exe_name = "speed-precise-gui.exe" if os.name == "nt" else "speed-precise-gui"
    for profile in ("debug", "release"):
        gui_bin = WORKSPACE_ROOT / "target" / profile / exe_name
        if not gui_bin.is_file():
            print(f"[FAIL] Expected compiled GUI binary at {gui_bin}")
            return False
        size_kb = gui_bin.stat().st_size / 1024.0
        print(f"       Binary ({profile:7s}): {gui_bin} ({size_kb:.1f} KiB)")

    if os.name == "nt":
        rel_gui = WORKSPACE_ROOT / "target" / "release" / "speed-precise-gui.exe"
        rel_cli = WORKSPACE_ROOT / "target" / "release" / "speed-precise-cli.exe"
        if not rel_cli.is_file():
            print(f"[FAIL] Expected compiled CLI release binary at {rel_cli}")
            return False
        print(f"       CLI    (release): {rel_cli} ({rel_cli.stat().st_size / 1024.0:.1f} KiB)")

        subsystem, dlls, rsrc_ids = _inspect_pe_subsystem_and_imports(rel_gui)
        # IMAGE_SUBSYSTEM_WINDOWS_GUI == 2 (no black console window on double-click)
        if subsystem != 2:
            print(f"[FAIL] Expected PE Subsystem 2 (WINDOWS_GUI) on release GUI, got {subsystem}")
            return False

        forbidden = [d for d in dlls if "vcruntime" in d.lower() or "msvcp" in d.lower()]
        if forbidden:
            print(f"[FAIL] Found non-portable dynamic VC++ runtime DLL dependencies: {forbidden}")
            return False

        for required_id, label in ((3, "RT_ICON"), (14, "RT_GROUP_ICON"), (16, "RT_VERSION")):
            if required_id not in rsrc_ids:
                print(
                    f"[FAIL] Missing embedded PE resource {label} ({required_id}) in {rel_gui}: found {rsrc_ids}"
                )
                return False

        print(f"       PE Subsystem    : {subsystem} (IMAGE_SUBSYSTEM_WINDOWS_GUI — zero console window)")
        print(f"       PE Resources    : {rsrc_ids} (RT_ICON + RT_GROUP_ICON + RT_VERSION verified)")
        print(f"       System DLLs Only: {', '.join(dlls)}")

    print("[PASS] Verified standalone portable `speed-precise-gui` & `speed-precise-cli` binaries")
    return True


def verify_step_log2_csv(csv_path: Path, expected_steps: int) -> bool:
    print(f"\n[STEP] Verify Generated Step Log 2 CSV Format (`{csv_path.name}`)")
    if not csv_path.is_file():
        print(f"[FAIL] CSV file was not created at {csv_path}")
        return False

    lines = [
        line.strip()
        for line in csv_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    expected_lines = expected_steps + 2  # Header + Row 0 + N steps
    if len(lines) != expected_lines:
        print(f"[FAIL] Expected {expected_lines} lines in CSV, found {len(lines)}")
        return False

    if lines[0] != "force, startX, endX, deltaX, deltaXDeg":
        print(f"[FAIL] Invalid CSV header: {lines[0]!r}")
        return False

    if lines[1] != "0, 0, 0, 0, 0":
        print(f"[FAIL] Invalid Row 0: {lines[1]!r}")
        return False

    for row_idx, line in enumerate(lines[2:], start=1):
        cols = [c.strip() for c in line.split(",")]
        if len(cols) != 5:
            print(f"[FAIL] Step {row_idx} malformed row: {line!r}")
            return False
        start_x = int(cols[1])
        end_x = int(cols[2])
        if abs(start_x) > 200:
            print(f"[FAIL] Step {row_idx} did not start near center (32768): startX={start_x}")
            return False
        if abs(end_x) > 8650:
            print(f"[FAIL] Step {row_idx} exceeded 85% (8500) soft-lock envelope: endX={end_x}")
            return False

    last_cols = [c.strip() for c in lines[-1].split(",")]
    if len(last_cols) != 5 or int(last_cols[0]) != 10000 or float(last_cols[4]) <= 0.0:
        print(f"[FAIL] Final step row invalid: {lines[-1]!r}")
        return False

    print(f"       Header   : {lines[0]}")
    print(f"       Row 0    : {lines[1]}")
    print(f"       Final Row: {lines[-1]}")
    print(f"[PASS] Verified {len(lines)} lines matching exact WheelCheck `log2.csv` format")
    return True


def verify_auto_tune_artifacts(lut_path: Path, ac_cfg_dir: Path, expected_lut_name: str) -> bool:
    print("\n[STEP] Verify Generated Inverse `.lut` & Assetto Corsa `ff_post_process.ini`")
    if not lut_path.is_file():
        print(f"[FAIL] LUT file was not created at {lut_path}")
        return False

    lut_lines = [
        line.strip()
        for line in lut_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    if len(lut_lines) != 101:
        print(f"[FAIL] Expected 101 lines in .lut file, found {len(lut_lines)}")
        return False

    if lut_lines[0] != "0|0":
        print(f"[FAIL] Expected first LUT line to be '0|0', found {lut_lines[0]!r}")
        return False

    if lut_lines[-1] != "1|1":
        print(f"[FAIL] Expected last LUT line to be '1|1', found {lut_lines[-1]!r}")
        return False

    prev_y = 0.0
    for idx, line in enumerate(lut_lines):
        parts = line.split("|")
        if len(parts) != 2:
            print(f"[FAIL] Malformed LUT line {idx}: {line!r}")
            return False
        _x_val = float(parts[0])
        y_val = float(parts[1])
        if y_val < prev_y - 1e-9:
            print(f"[FAIL] Non-monotonic LUT at line {idx}: {y_val} < {prev_y}")
            return False
        prev_y = y_val

    ac_lut_path = ac_cfg_dir / expected_lut_name
    ac_ini_path = ac_cfg_dir / "ff_post_process.ini"
    if not ac_lut_path.is_file():
        print(f"[FAIL] Injected AC LUT file missing at {ac_lut_path}")
        return False
    if not ac_ini_path.is_file():
        print(f"[FAIL] Injected AC ff_post_process.ini missing at {ac_ini_path}")
        return False

    ini_text = ac_ini_path.read_text(encoding="utf-8")
    for required_token in ("VERSION=1", "TYPE=LUT", "ENABLED=1", f"CURVE={expected_lut_name}"):
        if required_token not in ini_text:
            print(f"[FAIL] Missing token `{required_token}` in ff_post_process.ini")
            return False

    print(
        f"       LUT Lines: {len(lut_lines)} (Row[0]={lut_lines[0]}, Row[1]={lut_lines[1]}, Row[100]={lut_lines[-1]})"
    )
    print(
        f"       AC INI   : Verified [HEADER] (TYPE=LUT, ENABLED=1) & [LUT] (CURVE={expected_lut_name})"
    )
    print("[PASS] Verified `.lut` mathematical invariants & Assetto Corsa profile injection")
    return True


def _extract_ttf_cmap_codepoints(ttf_path: Path) -> set[int]:
    data = ttf_path.read_bytes()
    num_tables = struct.unpack(">H", data[4:6])[0]
    cmap_offset = None
    for i in range(num_tables):
        off = 12 + i * 16
        if data[off : off + 4] == b"cmap":
            cmap_offset = struct.unpack(">I", data[off + 8 : off + 12])[0]
            break
    if cmap_offset is None:
        return set()

    codepoints: set[int] = set()
    num_sub = struct.unpack(">H", data[cmap_offset + 2 : cmap_offset + 4])[0]
    for i in range(num_sub):
        sub_off = cmap_offset + 4 + i * 8
        fmt_off = cmap_offset + struct.unpack(">I", data[sub_off + 4 : sub_off + 8])[0]
        fmt = struct.unpack(">H", data[fmt_off : fmt_off + 2])[0]
        if fmt == 4:
            seg_count = struct.unpack(">H", data[fmt_off + 6 : fmt_off + 8])[0] // 2
            end_codes = struct.unpack(
                f">{seg_count}H",
                data[fmt_off + 14 : fmt_off + 14 + 2 * seg_count],
            )
            start_off = fmt_off + 16 + 2 * seg_count
            start_codes = struct.unpack(
                f">{seg_count}H",
                data[start_off : start_off + 2 * seg_count],
            )
            for start_cp, end_cp in zip(start_codes, end_codes):
                if start_cp != 0xFFFF:
                    codepoints.update(range(start_cp, end_cp + 1))
        elif fmt == 12:
            n_groups = struct.unpack(">I", data[fmt_off + 12 : fmt_off + 16])[0]
            for g in range(n_groups):
                goff = fmt_off + 16 + g * 12
                start_cp, end_cp, _ = struct.unpack(">III", data[goff : goff + 12])
                codepoints.update(range(start_cp, end_cp + 1))
    return codepoints


def verify_gui_font_glyph_coverage() -> bool:
    print("\n[STEP] Verify GUI Font Glyph Coverage Against `Ubuntu-Light.ttf` (`cmap` Audit)")
    cargo_src = Path.home() / ".cargo" / "registry" / "src"
    ubuntu_matches = list(cargo_src.glob("*/epaint_default_fonts-*/fonts/Ubuntu-Light.ttf"))
    if not ubuntu_matches:
        print("[FAIL] Could not locate `epaint_default_fonts` `Ubuntu-Light.ttf` in Cargo cache")
        return False

    ubuntu_cps = _extract_ttf_cmap_codepoints(ubuntu_matches[0])
    gui_src_dir = WORKSPACE_ROOT / "crates" / "speed-precise-gui" / "src"
    missing: list[str] = []
    checked_non_ascii: set[str] = set()

    for rs_path in sorted(gui_src_dir.glob("*.rs")):
        for lineno, line in enumerate(rs_path.read_text(encoding="utf-8").splitlines(), start=1):
            if line.lstrip().startswith("//"):
                continue
            for ch in line:
                if ord(ch) > 127:
                    checked_non_ascii.add(ch)
                    if ord(ch) not in ubuntu_cps:
                        nm = unicodedata.name(ch, "UNKNOWN")
                        missing.append(f"{rs_path.name}:{lineno} U+{ord(ch):04X} ({nm})")

    if missing:
        print(f"[FAIL] Found GUI characters missing from `Ubuntu-Light.ttf`: {missing[:10]}")
        return False

    print(
        f"       Audited {len(checked_non_ascii)} distinct non-ASCII codepoints across `speed-precise-gui/src/*.rs` — 0 missing glyphs"
    )
    print("[PASS] Verified 100% native `Ubuntu-Light.ttf` glyph coverage & vector UI icons")
    return True


def main() -> int:
    print("================================================================================")
    print(" Speed Precise — Master Regression Test Suite (Phase 3.2 Clean UX & Auto-LUT)")
    print("================================================================================")

    env = resolve_cargo_env()
    cargo = resolve_cargo_executable(env)

    temp_dir = Path(tempfile.gettempdir()) / f"SpeedPrecise_regression_{os.getpid()}"
    temp_ac_dir = temp_dir / "ac_cfg"
    temp_dir.mkdir(parents=True, exist_ok=True)
    temp_csv = temp_dir / "auto_log2.csv"
    temp_lut = temp_dir / "speed_precise.lut"

    steps: list[tuple[str, list[str]]] = [
        (
            "Cargo Workspace Compilation & Zero-Warning Check (core + cli + gui)",
            [cargo, "check", "--workspace", "--all-targets"],
        ),
        (
            "Cargo Workspace Unit, Smart Saturation, Cockpit Mapping, LUT & Bounded Plot Tests",
            [cargo, "test", "--workspace", "--", "--nocapture"],
        ),
        (
            "Native Desktop GUI Debug Binary Build (`speed-precise-gui`)",
            [cargo, "build", "-p", "speed-precise-gui"],
        ),
        (
            "Native Portable Workspace Release Build (`cargo build --release --workspace`)",
            [cargo, "build", "--release", "--workspace"],
        ),
        (
            "CLI Subcommand Help Verification",
            [cargo, "run", "--quiet", "-p", "speed-precise-cli", "--", "--help"],
        ),
        (
            "CLI DirectInput8 Live Enumeration Smoke Test (`list`)",
            [cargo, "run", "--quiet", "-p", "speed-precise-cli", "--", "list"],
        ),
        (
            "CLI Unified Auto-Tune Dry-Run Pipeline (`auto-tune --dry-run --inject-ac`)",
            [
                cargo,
                "run",
                "--quiet",
                "-p",
                "speed-precise-cli",
                "--",
                "auto-tune",
                "--dry-run",
                "--steps",
                "50",
                "--output-csv",
                str(temp_csv),
                "--output-lut",
                str(temp_lut),
                "--smoothing-window",
                "5",
                "--inject-ac",
                "--ac-cfg-dir",
                str(temp_ac_dir),
            ],
        ),
    ]

    passed = 0
    total_checks = len(steps) + 4
    try:
        for name, cmd in steps:
            if run_step(name, cmd, env=env):
                passed += 1
            else:
                print(
                    "\n================================================================================"
                )
                print(f" REGRESSION SUITE FAILED ({passed}/{total_checks} checks passed)")
                print(
                    "================================================================================"
                )
                return 1

        if verify_gui_binary_exists():
            passed += 1
        else:
            print("\n================================================================================")
            print(f" REGRESSION SUITE FAILED ({passed}/{total_checks} checks passed)")
            print("================================================================================")
            return 1

        if verify_step_log2_csv(temp_csv, expected_steps=50):
            passed += 1
        else:
            print("\n================================================================================")
            print(f" REGRESSION SUITE FAILED ({passed}/{total_checks} checks passed)")
            print("================================================================================")
            return 1

        if verify_auto_tune_artifacts(temp_lut, temp_ac_dir, expected_lut_name="speed_precise.lut"):
            passed += 1
        else:
            print("\n================================================================================")
            print(f" REGRESSION SUITE FAILED ({passed}/{total_checks} checks passed)")
            print("================================================================================")
            return 1

        if verify_gui_font_glyph_coverage():
            passed += 1
        else:
            print("\n================================================================================")
            print(f" REGRESSION SUITE FAILED ({passed}/{total_checks} checks passed)")
            print("================================================================================")
            return 1
    finally:
        if temp_dir.exists():
            shutil.rmtree(temp_dir, ignore_errors=True)

    print("\n================================================================================")
    print(f" ALL REGRESSION CHECKS PASSED ({passed}/{total_checks} checks passed)")
    print("================================================================================")
    return 0


if __name__ == "__main__":
    sys.exit(main())
