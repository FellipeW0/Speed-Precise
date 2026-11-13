#!/usr/bin/env python3
"""
Reproducible Portable Release Packager for Speed Precise (Windows x64).

Builds the optimized standalone workspace (`cargo build --release --workspace`),
verifies PE32+ portability invariants (`IMAGE_SUBSYSTEM_WINDOWS_GUI == 2` and zero
external VC++ runtime DLL dependencies), and packages:
  - `dist/SpeedPrecise/SpeedPrecise.exe`
  - `dist/SpeedPrecise/speed-precise-cli.exe`
  - `dist/SpeedPrecise/LEIA-ME.txt`
  - `dist/SpeedPrecise/README.md`
  - `dist/SpeedPrecise/LICENSE`
  - `dist/SpeedPrecise-v<VERSION>-win64-portable.zip`
  - `dist/SpeedPrecise-v<VERSION>-win64-portable.zip.sha256`
"""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import struct
import subprocess
import sys
import zipfile
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parent


def read_workspace_version(cargo_toml: Path) -> str:
    text = cargo_toml.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, flags=re.MULTILINE)
    if not match:
        raise RuntimeError(f"Could not read `[workspace.package].version` from {cargo_toml}")
    return match.group(1)


VERSION = read_workspace_version(WORKSPACE_ROOT / "Cargo.toml")
ZIP_BASENAME = f"SpeedPrecise-v{VERSION}-win64-portable.zip"

LEIA_ME_CONTENT = f"""================================================================================
 SPEED PRECISE v{VERSION} — GUIA RÁPIDO (WINDOWS x64 PORTÁTIL)
================================================================================

Aplicativo 100% portátil. Basta abrir o SpeedPrecise.exe com o volante conectado.

Não é necessário instalar drivers adicionais, .NET Framework ou pacotes do
Visual C++. O SpeedPrecise.exe comunica-se diretamente com o subsistema nativo
DirectInput8 (dinput8.dll) do Windows 10 / Windows 11.

--------------------------------------------------------------------------------
 1. COMO USAR — VOLANTE E PEDAIS (CALIBRAÇÃO NO DRIVER DO WINDOWS)
--------------------------------------------------------------------------------
  1. Conecte seu volante e/ou pedaleira USB ao PC.
  2. Abra `SpeedPrecise.exe`.
  3. Na aba "Volante e Pedais":
     - Ajuste o "Curso Máximo (Saturação)" (10% a 100%) caso seu pedal de freio
       seja rígido (load cell / borracha dura) e você deseje atingir 100% de
       frenagem nos simuladores com menor curso físico.
     - Ajuste a "Zona Morta Inicial" (0% a 30%) para eliminar pequenas leituras
       involuntárias ao apoiar o pé no pedal.
     - As alterações são gravadas diretamente na calibração nativa do Windows
       com backup automático de segurança em `%APPDATA%\\SpeedPrecise\\backups\\`.
     - Para voltar aos valores originais de fábrica a qualquer momento, clique
       em "Restaurar Padrão dos Pedais".

--------------------------------------------------------------------------------
 2. COMO USAR — GERADOR DE LUT (FORCE FEEDBACK 1000 Hz)
--------------------------------------------------------------------------------
  1. Centralize o volante, tire as mãos do aro e abra a aba
     "Gerador de LUT (Force Feedback)".
  2. Confirme o "Ângulo do Volante" configurado no software do fabricante
     (ex.: 900° no Logitech G HUB / Thrustmaster Control Panel).
  3. Clique em "Iniciar Teste e Gerar LUT". O volante executará pulsos
     automáticos de leitura (~58 segundos para 50 passos) e exibirá a curva LUT
     pronta na tela.
  4. PARADA DE EMERGÊNCIA IMEDIATA: Pressione ESPAÇO ou ESC no teclado (ou
     clique em "Parar Teste Agora") para cortar a força do motor em menos de
     1 milissegundo.
  5. Clique em "Aplicar no Assetto Corsa" para gravar e ativar automaticamente
     a curva no simulador (com backup de `ff_post_process.ini.bak`), ou clique
     em "Salvar Arquivo .LUT" para exportar para qualquer pasta.

--------------------------------------------------------------------------------
 3. ARQUIVOS INCLUÍDOS NESTE PACOTE
--------------------------------------------------------------------------------
  - SpeedPrecise.exe       : Aplicativo gráfico principal (portátil, sem console)
  - speed-precise-cli.exe  : Ferramenta opcional de linha de comando (CLI)
  - LEIA-ME.txt            : Este guia rápido em Português (PT-BR)
  - README.md              : Documentação completa do projeto
  - LICENSE                : Licença de software livre MIT
================================================================================
"""


def resolve_cargo() -> tuple[str, dict[str, str]]:
    env = os.environ.copy()
    cargo_bin_dir = Path.home() / ".cargo" / "bin"
    if cargo_bin_dir.is_dir():
        current_path = env.get("PATH", "")
        if str(cargo_bin_dir).lower() not in current_path.lower():
            env["PATH"] = f"{cargo_bin_dir}{os.pathsep}{current_path}"

    found = shutil.which("cargo", path=env.get("PATH"))
    if found:
        return found, env
    fallback = cargo_bin_dir / "cargo.exe"
    if fallback.is_file():
        return str(fallback), env
    return "cargo", env


def inspect_pe_portability(exe_path: Path) -> tuple[int, list[str]]:
    data = exe_path.read_bytes()
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\x00\x00":
        raise RuntimeError(f"Assinatura PE inválida em {exe_path}")

    num_sections = struct.unpack_from("<H", data, pe_off + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe_off + 20)[0]
    opt_off = pe_off + 24
    magic = struct.unpack_from("<H", data, opt_off)[0]
    subsystem = struct.unpack_from("<H", data, opt_off + 68)[0]

    dd_off = opt_off + (112 if magic == 0x20B else 96)
    import_rva, import_size = struct.unpack_from("<II", data, dd_off + 8)

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

    return subsystem, dlls


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    print("================================================================================")
    print(f" Speed Precise v{VERSION} — Portable Windows x64 Release Builder")
    print("================================================================================")

    cargo, env = resolve_cargo()
    # Keep `.cargo/config.toml` active (`+crt-static`) unless RUSTFLAGS already overrides it
    if "RUSTFLAGS" in env and "+crt-static" not in env["RUSTFLAGS"]:
        env["RUSTFLAGS"] = f"{env['RUSTFLAGS']} -C target-feature=+crt-static".strip()

    build_cmd = [cargo, "build", "--release", "--workspace"]
    print(f"\n[1/4] Compiling optimized release binaries: {' '.join(build_cmd)}")
    proc = subprocess.run(build_cmd, cwd=str(WORKSPACE_ROOT), env=env)
    if proc.returncode != 0:
        print(f"[ERROR] Release build failed with exit code {proc.returncode}")
        return proc.returncode

    src_gui = WORKSPACE_ROOT / "target" / "release" / "speed-precise-gui.exe"
    src_cli = WORKSPACE_ROOT / "target" / "release" / "speed-precise-cli.exe"
    src_readme = WORKSPACE_ROOT / "README.md"
    src_license = WORKSPACE_ROOT / "LICENSE"

    for required in (src_gui, src_cli, src_readme, src_license):
        if not required.is_file():
            print(f"[ERROR] Required artifact not found: {required}")
            return 1

    print("\n[2/4] Auditing PE32+ standalone portability (`SpeedPrecise.exe`)...")
    subsystem, imported_dlls = inspect_pe_portability(src_gui)
    if subsystem != 2:
        print(
            f"[ERROR] Expected PE Subsystem 2 (IMAGE_SUBSYSTEM_WINDOWS_GUI), got {subsystem}"
        )
        return 1
    non_portable = [
        d for d in imported_dlls if "vcruntime" in d.lower() or "msvcp" in d.lower()
    ]
    if non_portable:
        print(f"[ERROR] Found dynamic VC++ Redistributable DLL imports: {non_portable}")
        return 1
    print("      PE Subsystem : 2 (IMAGE_SUBSYSTEM_WINDOWS_GUI — no console window)")
    print(f"      OS DLLs Only : {', '.join(imported_dlls)}")

    dist_dir = WORKSPACE_ROOT / "dist"
    bundle_dir = dist_dir / "SpeedPrecise"
    if dist_dir.exists():
        shutil.rmtree(dist_dir)
    bundle_dir.mkdir(parents=True, exist_ok=True)

    dst_gui = bundle_dir / "SpeedPrecise.exe"
    dst_cli = bundle_dir / "speed-precise-cli.exe"
    dst_leiame = bundle_dir / "LEIA-ME.txt"
    dst_readme = bundle_dir / "README.md"
    dst_license = bundle_dir / "LICENSE"

    print("\n[3/4] Staging portable directory `dist/SpeedPrecise/`...")
    shutil.copy2(src_gui, dst_gui)
    shutil.copy2(src_cli, dst_cli)
    dst_leiame.write_text(LEIA_ME_CONTENT.replace("\n", "\r\n"), encoding="utf-8")
    shutil.copy2(src_readme, dst_readme)
    shutil.copy2(src_license, dst_license)

    zip_path = dist_dir / ZIP_BASENAME
    sha256_path = dist_dir / f"{ZIP_BASENAME}.sha256"

    print(f"\n[4/4] Compressing `{zip_path.name}` and generating SHA-256 checksum...")
    packaged_files = [dst_gui, dst_cli, dst_leiame, dst_readme, dst_license]
    with zipfile.ZipFile(
        zip_path, mode="w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as zf:
        for file_path in packaged_files:
            arcname = f"SpeedPrecise/{file_path.name}"
            zf.write(file_path, arcname=arcname)

    with zipfile.ZipFile(zip_path, mode="r") as zf:
        corrupt_entry = zf.testzip()
        if corrupt_entry is not None:
            print(f"[ERROR] Corrupt entry detected inside ZIP archive: {corrupt_entry}")
            return 1

    exe_sha256 = sha256_file(dst_gui)
    zip_sha256 = sha256_file(zip_path)
    sha256_path.write_text(f"{zip_sha256}  {ZIP_BASENAME}\n", encoding="utf-8")

    exe_bytes = dst_gui.stat().st_size
    cli_bytes = dst_cli.stat().st_size
    zip_bytes = zip_path.stat().st_size

    print("\n================================================================================")
    print(f" PORTABLE RELEASE PACKAGE READY — Speed Precise v{VERSION}")
    print("================================================================================")
    print(
        f" GUI Executable : {dst_gui}\n"
        f"                  {exe_bytes:,} bytes ({exe_bytes / (1024 * 1024):.2f} MiB / {exe_bytes / 1_000_000:.2f} MB)\n"
        f"                  SHA-256: {exe_sha256}"
    )
    print(
        f" CLI Executable : {dst_cli}\n"
        f"                  {cli_bytes:,} bytes ({cli_bytes / (1024 * 1024):.2f} MiB / {cli_bytes / 1_000_000:.2f} MB)"
    )
    print(
        f" Portable ZIP   : {zip_path}\n"
        f"                  {zip_bytes:,} bytes ({zip_bytes / (1024 * 1024):.2f} MiB / {zip_bytes / 1_000_000:.2f} MB)\n"
        f"                  SHA-256: {zip_sha256}"
    )
    print(f" Checksum File  : {sha256_path}")
    print("================================================================================")
    return 0


if __name__ == "__main__":
    sys.exit(main())
