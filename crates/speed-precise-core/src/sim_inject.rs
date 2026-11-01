//! Simulator Profile Injector (`sim_inject.rs`): Automatic `.lut` deployment and
//! non-destructive `ff_post_process.ini` configuration for Assetto Corsa and BeamNG.drive.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};
use crate::lut_math::LutResult;

/// Summary of an Assetto Corsa LUT profile injection operation.
#[derive(Debug, Clone)]
pub struct AssettoCorsaInjectionOutcome {
    pub cfg_dir: PathBuf,
    pub lut_path: PathBuf,
    pub ini_path: PathBuf,
    pub backup_ini_path: Option<PathBuf>,
    pub lut_filename: String,
}

/// Summary of a BeamNG.drive Response Correction Curve (`.lut`) export operation.
#[derive(Debug, Clone)]
pub struct BeamNgInjectionOutcome {
    pub inputmaps_dir: PathBuf,
    pub lut_path: PathBuf,
    pub lut_filename: String,
}

/// Resolves the default Assetto Corsa configuration directory:
/// `%USERPROFILE%\Documents\Assetto Corsa\cfg`.
pub fn default_assetto_corsa_cfg_dir() -> PathBuf {
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        PathBuf::from(user_profile)
            .join("Documents")
            .join("Assetto Corsa")
            .join("cfg")
    } else {
        std::env::temp_dir().join("Assetto Corsa").join("cfg")
    }
}

/// Resolves the default BeamNG.drive `inputmaps` directory:
/// `%LOCALAPPDATA%\BeamNG.drive\latest\settings\inputmaps`.
pub fn default_beamng_inputmaps_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data)
            .join("BeamNG.drive")
            .join("latest")
            .join("settings")
            .join("inputmaps")
    } else if let Ok(user_profile) = std::env::var("USERPROFILE") {
        PathBuf::from(user_profile)
            .join("AppData")
            .join("Local")
            .join("BeamNG.drive")
            .join("latest")
            .join("settings")
            .join("inputmaps")
    } else {
        std::env::temp_dir()
            .join("BeamNG.drive")
            .join("latest")
            .join("settings")
            .join("inputmaps")
    }
}

/// Updates an existing `ff_post_process.ini` text (or generates a fresh one when `None`)
/// so that:
/// - `[HEADER]` contains `VERSION=1`, `TYPE=LUT`, `ENABLED=1`
/// - `[LUT]` contains `CURVE=<lut_filename>`
/// while preserving any other sections (`[GAMMA]`, etc.) and comments.
pub fn update_ff_post_process_ini(existing_ini: Option<&str>, lut_filename: &str) -> String {
    let raw = match existing_ini {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return format!(
                "[HEADER]\nVERSION=1\nTYPE=LUT\nENABLED=1\n\n[GAMMA]\nVALUE=1\n\n[LUT]\nCURVE={lut_filename}\n"
            );
        }
    };

    // Parse sections while preserving order and unknown sections/keys
    #[derive(Default)]
    struct IniSection {
        header: String,
        lines: Vec<String>,
    }

    let mut sections: Vec<IniSection> = Vec::new();
    let mut current = IniSection {
        header: String::new(),
        lines: Vec::new(),
    };

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !current.header.is_empty() || !current.lines.is_empty() {
                sections.push(current);
            }
            current = IniSection {
                header: trimmed.to_uppercase(),
                lines: Vec::new(),
            };
        } else {
            current.lines.push(line.to_string());
        }
    }
    if !current.header.is_empty() || !current.lines.is_empty() {
        sections.push(current);
    }

    let mut found_header = false;
    let mut found_lut = false;

    for sec in &mut sections {
        if sec.header == "[HEADER]" {
            found_header = true;
            upsert_ini_key(&mut sec.lines, "VERSION", "1");
            upsert_ini_key(&mut sec.lines, "TYPE", "LUT");
            upsert_ini_key(&mut sec.lines, "ENABLED", "1");
        } else if sec.header == "[LUT]" {
            found_lut = true;
            upsert_ini_key(&mut sec.lines, "CURVE", lut_filename);
        }
    }

    if !found_header {
        sections.insert(
            0,
            IniSection {
                header: "[HEADER]".to_string(),
                lines: vec![
                    "VERSION=1".to_string(),
                    "TYPE=LUT".to_string(),
                    "ENABLED=1".to_string(),
                ],
            },
        );
    }

    if !found_lut {
        sections.push(IniSection {
            header: "[LUT]".to_string(),
            lines: vec![format!("CURVE={lut_filename}")],
        });
    }

    let mut out = String::with_capacity(raw.len() + 128);
    for (idx, sec) in sections.iter().enumerate() {
        if idx > 0 && !out.ends_with("\n\n") {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        if !sec.header.is_empty() {
            let _ = writeln!(out, "{}", sec.header);
        }
        for l in &sec.lines {
            if !l.trim().is_empty() {
                let _ = writeln!(out, "{l}");
            }
        }
    }

    out
}

fn upsert_ini_key(lines: &mut Vec<String>, key: &str, value: &str) {
    let target_upper = key.to_uppercase();
    for line in lines.iter_mut() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim().to_uppercase() == target_upper {
                *line = format!("{key}={value}");
                return;
            }
        }
    }
    lines.push(format!("{key}={value}"));
}

/// Writes `<lut_filename>` into the target Assetto Corsa `cfg` directory, backs up any
/// existing `ff_post_process.ini`, and updates `ff_post_process.ini` to activate the LUT.
pub fn inject_assetto_corsa_lut(
    lut: &LutResult,
    lut_filename: &str,
    custom_cfg_dir: Option<&Path>,
) -> CoreResult<AssettoCorsaInjectionOutcome> {
    let cfg_dir = match custom_cfg_dir {
        Some(dir) => dir.to_path_buf(),
        None => default_assetto_corsa_cfg_dir(),
    };

    fs::create_dir_all(&cfg_dir).map_err(|source| CoreError::SimInjectIo {
        path: cfg_dir.display().to_string(),
        source,
    })?;

    let clean_name = Path::new(lut_filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("speed_precise.lut")
        .to_string();

    let lut_path = cfg_dir.join(&clean_name);
    fs::write(&lut_path, lut.to_lut_string()).map_err(|source| CoreError::SimInjectIo {
        path: lut_path.display().to_string(),
        source,
    })?;

    let ini_path = cfg_dir.join("ff_post_process.ini");
    let mut backup_ini_path = None;
    let existing_content = if ini_path.is_file() {
        let content = fs::read_to_string(&ini_path).map_err(|source| CoreError::SimInjectIo {
            path: ini_path.display().to_string(),
            source,
        })?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak_path = cfg_dir.join(format!("ff_post_process_{ts}.ini.bak"));
        fs::write(&bak_path, &content).map_err(|source| CoreError::SimInjectIo {
            path: bak_path.display().to_string(),
            source,
        })?;
        backup_ini_path = Some(bak_path);
        Some(content)
    } else {
        None
    };

    let updated_ini = update_ff_post_process_ini(existing_content.as_deref(), &clean_name);
    fs::write(&ini_path, updated_ini).map_err(|source| CoreError::SimInjectIo {
        path: ini_path.display().to_string(),
        source,
    })?;

    Ok(AssettoCorsaInjectionOutcome {
        cfg_dir,
        lut_path,
        ini_path,
        backup_ini_path,
        lut_filename: clean_name,
    })
}

/// Exports `<lut_filename>` (default `wheel.lut`) into the BeamNG.drive `inputmaps` directory
/// (`%LOCALAPPDATA%\BeamNG.drive\latest\settings\inputmaps` or custom path) for direct use by
/// BeamNG's Force Feedback Response Correction Curve engine.
pub fn inject_beamng_lut(
    lut: &LutResult,
    lut_filename: &str,
    custom_inputmaps_dir: Option<&Path>,
) -> CoreResult<BeamNgInjectionOutcome> {
    let inputmaps_dir = match custom_inputmaps_dir {
        Some(dir) => dir.to_path_buf(),
        None => default_beamng_inputmaps_dir(),
    };

    fs::create_dir_all(&inputmaps_dir).map_err(|source| CoreError::SimInjectIo {
        path: inputmaps_dir.display().to_string(),
        source,
    })?;

    let clean_name = Path::new(lut_filename)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("wheel.lut")
        .to_string();

    let lut_path = inputmaps_dir.join(&clean_name);
    fs::write(&lut_path, lut.to_lut_string()).map_err(|source| CoreError::SimInjectIo {
        path: lut_path.display().to_string(),
        source,
    })?;

    Ok(BeamNgInjectionOutcome {
        inputmaps_dir,
        lut_path,
        lut_filename: clean_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lut_math::{generate_inverse_lut, LutConfig, StepLogSample};

    #[test]
    fn test_update_ff_post_process_ini_preserves_gamma_and_comments() {
        let existing = "\
[HEADER]
VERSION=1
TYPE=GAMMA
ENABLED=0

[GAMMA]
VALUE=0.85
; User custom gamma comment

[LUT]
CURVE=old_wheel.lut
";
        let updated = update_ff_post_process_ini(Some(existing), "speed_precise.lut");
        assert!(updated.contains("TYPE=LUT"));
        assert!(updated.contains("ENABLED=1"));
        assert!(updated.contains("VERSION=1"));
        assert!(updated.contains("[GAMMA]"));
        assert!(updated.contains("VALUE=0.85"));
        assert!(updated.contains("; User custom gamma comment"));
        assert!(updated.contains("CURVE=speed_precise.lut"));
        assert!(!updated.contains("CURVE=old_wheel.lut"));
    }

    #[test]
    fn test_inject_assetto_corsa_lut_creates_backup_and_files() {
        let temp_dir = std::env::temp_dir()
            .join("SpeedPrecise_test_ac_inject")
            .join(format!("run_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Pre-populate an existing ff_post_process.ini to verify backup creation
        let initial_ini = "[HEADER]\nVERSION=1\nTYPE=GAMMA\nENABLED=0\n\n[GAMMA]\nVALUE=0.5\n";
        fs::write(temp_dir.join("ff_post_process.ini"), initial_ini).unwrap();

        let samples = vec![
            StepLogSample {
                force: 0,
                start_x: 0,
                end_x: 0,
                delta_x: 0,
                delta_x_deg: 0.0,
            },
            StepLogSample {
                force: 5_000,
                start_x: 0,
                end_x: 1_000,
                delta_x: 1_000,
                delta_x_deg: 45.0,
            },
            StepLogSample {
                force: 10_000,
                start_x: 0,
                end_x: 2_000,
                delta_x: 2_000,
                delta_x_deg: 90.0,
            },
        ];
        let lut = generate_inverse_lut(&samples, &LutConfig::default()).unwrap();

        let outcome =
            inject_assetto_corsa_lut(&lut, "custom_wheel.lut", Some(&temp_dir)).unwrap();
        assert!(outcome.lut_path.is_file());
        assert!(outcome.ini_path.is_file());
        assert!(outcome.backup_ini_path.as_ref().unwrap().is_file());

        let ini_text = fs::read_to_string(&outcome.ini_path).unwrap();
        assert!(ini_text.contains("TYPE=LUT"));
        assert!(ini_text.contains("ENABLED=1"));
        assert!(ini_text.contains("VALUE=0.5"));
        assert!(ini_text.contains("CURVE=custom_wheel.lut"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_inject_beamng_lut_writes_valid_lut_file() {
        let temp_dir = std::env::temp_dir()
            .join("SpeedPrecise_test_beamng_inject")
            .join(format!("run_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let samples = vec![
            StepLogSample {
                force: 0,
                start_x: 0,
                end_x: 0,
                delta_x: 0,
                delta_x_deg: 0.0,
            },
            StepLogSample {
                force: 5_000,
                start_x: 0,
                end_x: 1_200,
                delta_x: 1_200,
                delta_x_deg: 54.0,
            },
            StepLogSample {
                force: 10_000,
                start_x: 0,
                end_x: 2_400,
                delta_x: 2_400,
                delta_x_deg: 108.0,
            },
        ];
        let lut = generate_inverse_lut(&samples, &LutConfig::default()).unwrap();

        let outcome = inject_beamng_lut(&lut, "wheel.lut", Some(&temp_dir)).unwrap();
        assert!(outcome.lut_path.is_file());
        assert_eq!(outcome.lut_filename, "wheel.lut");

        let content = fs::read_to_string(&outcome.lut_path).unwrap();
        assert!(content.starts_with("0|0\n"));
        assert!(content.ends_with("1|1\n"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
