//! Build script for `speed-precise-gui` embedding the official Windows application icon
//! (`Speed_Precise.ico`) and `VS_VERSION_INFO` metadata into `SpeedPrecise.exe` (`RT_ICON` / `.rsrc`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set"));
    let ico_path = manifest_dir.join("..").join("..").join("Speed_Precise.ico");

    println!("cargo:rerun-if-changed={}", ico_path.display());
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=PATH");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env != "msvc" {
        return;
    }

    if !ico_path.is_file() {
        panic!(
            "Official application icon not found at `{}`",
            ico_path.display()
        );
    }

    let rc_exe = locate_rc_executable(&env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default())
        .expect("Failed to locate Windows SDK `rc.exe` to embed Speed_Precise.ico");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set"));
    let rc_file = out_dir.join("app_icon.rc");
    let res_file = out_dir.join("app_icon.res");

    let canonical_ico = fs::canonicalize(&ico_path).unwrap_or(ico_path);
    let ico_str = canonical_ico
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "\\\\");

    let major = env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_else(|_| "1".to_string());
    let minor = env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_else(|_| "0".to_string());
    let patch = env::var("CARGO_PKG_VERSION_PATCH").unwrap_or_else(|_| "1".to_string());
    let version_str = format!("{major}.{minor}.{patch}");
    let file_version_comma = format!("{major},{minor},{patch},0");

    // Resource ID `1` guarantees Windows Explorer selects this icon as the primary EXE icon.
    let rc_script = format!(
        r#"1 ICON "{ico_str}"

1 VERSIONINFO
FILEVERSION {file_version_comma}
PRODUCTVERSION {file_version_comma}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Speed Precise Engineering\0"
            VALUE "FileDescription", "Speed Precise - DirectInput8 Calibration, FFB Step Log 2 & LUT Studio\0"
            VALUE "FileVersion", "{version_str}.0\0"
            VALUE "InternalName", "SpeedPrecise\0"
            VALUE "LegalCopyright", "Copyright (c) 2026 Speed Precise Contributors (MIT)\0"
            VALUE "OriginalFilename", "SpeedPrecise.exe\0"
            VALUE "ProductName", "Speed Precise\0"
            VALUE "ProductVersion", "{version_str}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    fs::write(&rc_file, rc_script).expect("Failed to write temporary app_icon.rc");

    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .status()
        .unwrap_or_else(|err| panic!("Failed to invoke `{}`: {err}", rc_exe.display()));

    if !status.success() {
        panic!(
            "`rc.exe` failed with exit code {:?} while compiling `{}`",
            status.code(),
            rc_file.display()
        );
    }

    // Link strictly into binary targets (`speed-precise-gui.exe`) so test harnesses avoid conflicts.
    println!("cargo:rustc-link-arg-bins={}", res_file.display());
}

fn locate_rc_executable(target_arch: &str) -> Option<PathBuf> {
    if let Some(rc_env) = env::var_os("RC") {
        let candidate = PathBuf::from(rc_env);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let sdk_arch = match target_arch {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };

    let mut kit_roots = Vec::new();
    for env_key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(base) = env::var_os(env_key) {
            let base_path = Path::new(&base);
            kit_roots.push(base_path.join("Windows Kits").join("10").join("bin"));
            kit_roots.push(base_path.join("Windows Kits").join("11").join("bin"));
        }
    }
    if let Some(reg_root) = query_kits_root_from_registry() {
        kit_roots.push(reg_root.join("bin"));
    }

    let mut candidates: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for bin_root in kit_roots {
        let Ok(entries) = fs::read_dir(&bin_root) else {
            continue;
        };
        for entry in entries.flatten() {
            let ver_dir = entry.path();
            if !ver_dir.is_dir() {
                continue;
            }
            let rc_candidate = ver_dir.join(sdk_arch).join("rc.exe");
            if rc_candidate.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let key: Vec<u32> = name
                    .split('.')
                    .map(|part| part.parse::<u32>().unwrap_or(0))
                    .collect();
                candidates.push((key, rc_candidate));
            }
        }
        let direct_candidate = bin_root.join(sdk_arch).join("rc.exe");
        if direct_candidate.is_file() {
            candidates.push((vec![0], direct_candidate));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().next().map(|(_, path)| path)
}

fn query_kits_root_from_registry() -> Option<PathBuf> {
    for key in [
        r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots",
    ] {
        let Ok(output) = Command::new("reg")
            .args(["query", key, "/v", "KitsRoot10"])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(idx) = line.find("KitsRoot10") {
                let tail = &line[idx + "KitsRoot10".len()..];
                if let Some(reg_sz) = tail.split("REG_SZ").nth(1) {
                    let path = Path::new(reg_sz.trim());
                    if path.is_dir() {
                        return Some(path.to_path_buf());
                    }
                }
            }
        }
    }
    None
}
