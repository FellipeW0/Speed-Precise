//! LUT Mathematical Engine (`lut_math.rs`): WheelCheck CSV parser, spike & monotonicity
//! filter, deadzone floor detector, symmetric boundary-preserving moving average smoother,
//! piecewise linear function inverter ($x = f^{-1}(y)$), `.lut` formatter, and ASCII chart renderer.
//!
//! Verified against the Phase 1 CIL bytecode audit of `LUTGenerator.exe` (`0x06000008` & `0x0600000E`)
//! and `LUTLibrary.dll` tokens (`0x0A000024`–`0x0A00005A`).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
pub use crate::ffb_engine::StepLog2Sample as StepLogSample;

/// Default number of steps in generated `.lut` files (`100` steps -> `101` points `0.00..=1.00`).
pub const DEFAULT_LUT_STEPS: usize = 100;
/// Default moving-average window size (`5`).
pub const DEFAULT_SMOOTHING_WINDOW: usize = 5;
/// Default deadzone detection threshold as a fraction of maximum displacement (`0.5%` = `0.005`).
pub const DEFAULT_DEADZONE_THRESHOLD_RATIO: f64 = 0.005;

/// Configuration parameters for the inverse LUT mathematical pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LutConfig {
    /// Number of uniform output intervals in `[0.0, 1.0]` (default: `100` -> `101` lines).
    pub output_steps: usize,
    /// Symmetric moving-average window size for response curve smoothing (default: `5`).
    pub smoothing_window: usize,
    /// Relative displacement threshold (`0.0..0.10`) below which movement is treated as deadzone noise.
    pub deadzone_threshold_ratio: f64,
    /// Optional preload cap (`0.0..=1.0`) limiting the initial deadzone jump at step 1.
    pub center_preload_cap: Option<f64>,
}

impl Default for LutConfig {
    fn default() -> Self {
        Self {
            output_steps: DEFAULT_LUT_STEPS,
            smoothing_window: DEFAULT_SMOOTHING_WINDOW,
            deadzone_threshold_ratio: DEFAULT_DEADZONE_THRESHOLD_RATIO,
            center_preload_cap: None,
        }
    }
}

impl LutConfig {
    pub fn validate(&self) -> CoreResult<()> {
        if self.output_steps < 2 || self.output_steps > 10_000 {
            return Err(CoreError::InvalidLutConfig {
                reason: "`output_steps` must be between 2 and 10000",
            });
        }
        if !(0.0..=0.25).contains(&self.deadzone_threshold_ratio) {
            return Err(CoreError::InvalidLutConfig {
                reason: "`deadzone_threshold_ratio` must be in 0.0..=0.25",
            });
        }
        if let Some(cap) = self.center_preload_cap {
            if !(0.0..=1.0).contains(&cap) {
                return Err(CoreError::InvalidLutConfig {
                    reason: "`center_preload_cap` must be in 0.0..=1.0",
                });
            }
        }
        Ok(())
    }
}

/// Single `(x, y)` coordinate pair in an inverse Force Feedback `.lut` table.
/// - `x`: Normalized game FFB demand $\in [0.0, 1.0]$
/// - `y`: Corrected motor torque output $\in [0.0, 1.0]$
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LutPoint {
    pub x: f64,
    pub y: f64,
}

/// Complete output of the `generate_inverse_lut` mathematical pipeline, including all
/// intermediate curves required by CLI reporting and Phase 3 GUI plotting (`wcraw`, `wcsmoothed`, `lutoutput`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LutResult {
    /// Final inverted LUT points (`output_steps + 1` entries from `(0.0, 0.0)` to `(1.0, 1.0)`).
    pub points: Vec<LutPoint>,
    /// Normalized raw WheelCheck response curve `(force_norm, raw_disp_norm)` (`wcraw`).
    pub raw_response_norm: Vec<(f64, f64)>,
    /// Normalized response after spike & monotonicity filtering (`getDeltaX`).
    pub filtered_response_norm: Vec<(f64, f64)>,
    /// Normalized response after symmetric boundary-preserving smoothing (`wcsmoothed` / `getRealForce`).
    pub smoothed_response_norm: Vec<(f64, f64)>,
    /// Detected deadzone floor in normalized force units (`0.0..=1.0`).
    pub deadzone_floor_norm: f64,
    /// Detected deadzone floor in percentage (`0.0..=100.0%`).
    pub deadzone_percent: f64,
    /// Count of samples corrected by the spike/monotonicity filter (`correctedValues` in `LUTGenerator`).
    pub corrected_spikes_count: usize,
    /// Maximum input force from the CSV (`10000` in standard WheelCheck logs).
    pub max_force: i32,
    /// Peak displacement in degrees (`R_max`).
    pub max_displacement_deg: f64,
}

impl LutResult {
    /// Formats the LUT points into standard Assetto Corsa / BeamNG `x|y` text format,
    /// rounding values to 3 decimal places (`Math.Round(..., 3)`).
    pub fn to_lut_string(&self) -> String {
        let mut out = String::with_capacity(self.points.len() * 14);
        for pt in &self.points {
            let x_str = format_lut_number(pt.x);
            let y_str = format_lut_number(pt.y);
            let _ = writeln!(out, "{x_str}|{y_str}");
        }
        out
    }
}

/// Formats a normalized LUT number rounded to 3 decimal places (`0.00`..`1.00` for `x`, up to 3 decimals for `y`).
#[inline]
pub fn round_3(val: f64) -> f64 {
    (val * 1_000.0).round() / 1_000.0
}

fn format_lut_number(val: f64) -> String {
    let r = round_3(val.clamp(0.0, 1.0));
    if (r - 0.0).abs() < 1e-9 {
        "0".to_string()
    } else if (r - 1.0).abs() < 1e-9 {
        "1".to_string()
    } else {
        // Trim trailing zeros while keeping up to 3 decimal places (matches CultureInfo("en-GB") in LUTGenerator.exe)
        let s = format!("{r:.3}");
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

/// Parses a `WheelCheck.exe` `Step Log 2` CSV string into a vector of [`StepLogSample`].
///
/// Supports standard comma-separated headers (`force, startX, endX, deltaX, deltaXDeg`),
/// whitespace variations, and both integer and float representations of `startX`, `endX`, `deltaX`.
pub fn parse_wheelcheck_csv(csv_text: &str) -> CoreResult<Vec<StepLogSample>> {
    let mut samples: Vec<StepLogSample> = Vec::new();

    for (line_idx, raw_line) in csv_text.lines().enumerate() {
        let line_num = line_idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Skip header line if it starts with alphabetic characters (e.g. "force, startX, ...")
        if trimmed
            .chars()
            .next()
            .map(|c| c.is_alphabetic())
            .unwrap_or(false)
        {
            continue;
        }

        let cols: Vec<&str> = trimmed.split(',').map(|c| c.trim()).collect();
        if cols.len() < 5 {
            return Err(CoreError::CsvParse {
                line: line_num,
                reason: format!("Expected 5 comma-separated columns, found {}", cols.len()),
            });
        }

        let force = parse_i32_or_float(cols[0], line_num, "force")?;
        let start_x = parse_i32_or_float(cols[1], line_num, "startX")?;
        let end_x = parse_i32_or_float(cols[2], line_num, "endX")?;
        let delta_x = parse_i32_or_float(cols[3], line_num, "deltaX")?;
        let delta_x_deg = cols[4].parse::<f64>().map_err(|e| CoreError::CsvParse {
            line: line_num,
            reason: format!("Invalid float for `deltaXDeg` (`{}`): {e}", cols[4]),
        })?;

        samples.push(StepLogSample {
            force,
            start_x,
            end_x,
            delta_x,
            delta_x_deg,
        });
    }

    if samples.is_empty() {
        return Err(CoreError::EmptyStepLogCsv);
    }

    // Ensure row 0 (origin 0, 0, 0, 0, 0) is present at index 0
    if samples[0].force > 0 {
        samples.insert(0, StepLogSample::ZERO);
    }

    Ok(samples)
}

fn parse_i32_or_float(token: &str, line: usize, col_name: &str) -> CoreResult<i32> {
    if let Ok(v) = token.parse::<i32>() {
        return Ok(v);
    }
    if let Ok(f) = token.parse::<f64>() {
        return Ok(f.round() as i32);
    }
    Err(CoreError::CsvParse {
        line,
        reason: format!("Invalid number for `{col_name}` (`{token}`)"),
    })
}

/// Reads and parses a `WheelCheck` `log2.csv` file from disk.
pub fn read_wheelcheck_csv_file(path: &Path) -> CoreResult<Vec<StepLogSample>> {
    let text = fs::read_to_string(path).map_err(|source| CoreError::LutIo {
        path: path.display().to_string(),
        source,
    })?;
    parse_wheelcheck_csv(&text)
}

/// Writes a [`LutResult`] to disk as a `.lut` file.
pub fn write_lut_file(path: &Path, lut: &LutResult) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| CoreError::LutIo {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    fs::write(path, lut.to_lut_string()).map_err(|source| CoreError::LutIo {
        path: path.display().to_string(),
        source,
    })
}

/// Computes the complete inverse Force Feedback LUT from a slice of [`StepLogSample`] rows.
///
/// Stages:
/// 1. Displacement extraction (`deltaXDeg`, or fallback to `deltaX`) & normalization.
/// 2. Single-sample spike filtering & strict non-decreasing monotonicity enforcement (`corrected_spikes_count`).
/// 3. Deadzone floor detection (`deadzone_floor_norm`) where normalized movement $\le \text{threshold}$.
/// 4. Symmetric boundary-preserving moving-average smoothing over the active response region.
/// 5. Piecewise linear inversion $x = f^{-1}(y)$ over `config.output_steps` uniform intervals in $[0.0, 1.0]$.
pub fn generate_inverse_lut(
    samples: &[StepLogSample],
    config: &LutConfig,
) -> CoreResult<LutResult> {
    config.validate()?;
    if samples.len() < 2 {
        return Err(CoreError::EmptyStepLogCsv);
    }

    let n = samples.len();
    let max_force = samples
        .iter()
        .map(|s| s.force.abs())
        .max()
        .unwrap_or(10_000)
        .max(1);

    // Determine whether `delta_x_deg` has positive span; fallback to `delta_x` if `delta_x_deg` is all zero.
    let max_deg_raw = samples
        .iter()
        .map(|s| s.delta_x_deg.abs())
        .fold(0.0_f64, f64::max);
    let use_deg = max_deg_raw > 1e-9;

    let mut f_norm = Vec::with_capacity(n);
    let mut r_raw = Vec::with_capacity(n);
    for (i, s) in samples.iter().enumerate() {
        let fn_i = if i == 0 {
            0.0
        } else if i + 1 == n {
            1.0
        } else {
            ((s.force.abs() as f64) / (max_force as f64)).clamp(0.0, 1.0)
        };
        let disp = if use_deg {
            s.delta_x_deg.abs()
        } else {
            s.delta_x.abs() as f64
        };
        f_norm.push(fn_i);
        r_raw.push(if i == 0 { 0.0 } else { disp });
    }

    let peak_raw = r_raw.iter().copied().fold(0.0_f64, f64::max);
    if peak_raw <= 1e-9 {
        return Err(CoreError::ZeroMaxDisplacement);
    }

    // Stage 1: Spike filtering & monotonicity enforcement
    let mut r_filt = r_raw.clone();
    r_filt[0] = 0.0;

    // Pass 1a: Despike single-point upward/downward outliers before cumulative max
    for i in 1..(n - 1) {
        let prev = r_filt[i - 1];
        let cur = r_filt[i];
        let next = r_filt[i + 1];
        if prev <= next && (cur > next || cur < prev) {
            r_filt[i] = 0.5 * (prev + next);
        }
    }

    // Pass 1b: Enforce non-decreasing monotonicity
    for i in 1..n {
        if r_filt[i] < r_filt[i - 1] {
            r_filt[i] = r_filt[i - 1];
        }
    }

    let mut corrected_spikes_count = 0usize;
    for i in 0..n {
        if (r_filt[i] - r_raw[i]).abs() > 1e-5 {
            corrected_spikes_count += 1;
        }
    }

    let r_max = r_filt[n - 1].max(peak_raw);
    let raw_response_norm: Vec<(f64, f64)> = (0..n)
        .map(|i| (f_norm[i], (r_raw[i] / r_max).clamp(0.0, 1.0)))
        .collect();

    let mut y_filt: Vec<f64> = (0..n)
        .map(|i| (r_filt[i] / r_max).clamp(0.0, 1.0))
        .collect();
    y_filt[0] = 0.0;
    y_filt[n - 1] = 1.0;

    // Stage 2: Deadzone floor detection
    // Find the last index `dz_idx` where normalized movement remains <= deadzone_threshold_ratio
    let mut dz_idx = 0usize;
    for i in 0..(n - 1) {
        if y_filt[i] <= config.deadzone_threshold_ratio {
            dz_idx = i;
        } else {
            break;
        }
    }

    // Zero out sub-threshold movement inside the deadzone floor [0..=dz_idx]
    for y in y_filt.iter_mut().take(dz_idx + 1) {
        *y = 0.0;
    }

    let deadzone_floor_norm = f_norm[dz_idx];
    let deadzone_percent = deadzone_floor_norm * 100.0;

    let filtered_response_norm: Vec<(f64, f64)> =
        (0..n).map(|i| (f_norm[i], y_filt[i])).collect();

    // Stage 3: Symmetric boundary-preserving moving-average smoothing over active range [dz_idx..n]
    let mut y_smooth = y_filt.clone();
    let half_win = config.smoothing_window / 2;
    if half_win > 0 && (dz_idx + 2) < n {
        for i in (dz_idx + 1)..(n - 1) {
            // Symmetric radius clamped to stay within [dz_idx, n - 1]
            // Preserves linear segments identically and keeps y_smooth[dz_idx] == 0.0 sharp!
            let r = half_win.min(i - dz_idx).min((n - 1) - i);
            let mut sum = 0.0;
            let count = (2 * r + 1) as f64;
            for k in (i - r)..=(i + r) {
                sum += y_filt[k];
            }
            y_smooth[i] = (sum / count).clamp(0.0, 1.0);
        }
    }

    // Ensure strict non-decreasing monotonicity and exact endpoints on y_smooth
    y_smooth[0] = 0.0;
    for y in y_smooth.iter_mut().take(dz_idx + 1) {
        *y = 0.0;
    }
    for i in (dz_idx + 1)..n {
        if y_smooth[i] < y_smooth[i - 1] {
            y_smooth[i] = y_smooth[i - 1];
        }
    }
    y_smooth[n - 1] = 1.0;

    let smoothed_response_norm: Vec<(f64, f64)> =
        (0..n).map(|i| (f_norm[i], y_smooth[i])).collect();

    // Stage 4: Piecewise Linear Inversion x = f^-1(y)
    let steps = config.output_steps;
    let mut points = Vec::with_capacity(steps + 1);
    let mut prev_y_rounded = 0.0_f64;

    for k in 0..=steps {
        if k == 0 {
            points.push(LutPoint { x: 0.0, y: 0.0 });
            prev_y_rounded = 0.0;
            continue;
        }
        if k == steps {
            points.push(LutPoint { x: 1.0, y: 1.0 });
            break;
        }

        let target_y = (k as f64) / (steps as f64);

        // Locate the first active segment (i - 1, i) where i > dz_idx and y_smooth[i] >= target_y
        let mut seg_idx = (dz_idx + 1).min(n - 1);
        for i in (dz_idx + 1)..n {
            if y_smooth[i] >= target_y && y_smooth[i] > y_smooth[i - 1] {
                seg_idx = i;
                break;
            }
        }

        let y0 = y_smooth[seg_idx - 1];
        let y1 = y_smooth[seg_idx];
        let x0 = f_norm[seg_idx - 1];
        let x1 = f_norm[seg_idx];

        let mut inv_force = if (y1 - y0).abs() > 1e-12 {
            x0 + ((target_y - y0) / (y1 - y0)) * (x1 - x0)
        } else {
            x1
        };

        if let Some(cap) = config.center_preload_cap {
            if k == 1 && inv_force > cap {
                inv_force = cap;
            }
        }

        let x_rounded = round_3(target_y);
        let y_rounded = round_3(inv_force.clamp(0.0, 1.0)).max(prev_y_rounded);
        prev_y_rounded = y_rounded;

        points.push(LutPoint {
            x: x_rounded,
            y: y_rounded,
        });
    }

    Ok(LutResult {
        points,
        raw_response_norm,
        filtered_response_norm,
        smoothed_response_norm,
        deadzone_floor_norm,
        deadzone_percent,
        corrected_spikes_count,
        max_force,
        max_displacement_deg: r_max,
    })
}

/// Renders an ASCII chart comparing the Raw WheelCheck Response (`r`), Linear Reference (`.`),
/// and Corrected Inverse LUT (`#`) for terminal preview.
pub fn render_ascii_lut_chart(lut: &LutResult, width: usize, height: usize) -> String {
    let w = width.clamp(20, 100);
    let h = height.clamp(10, 40);
    let mut grid = vec![vec![' '; w]; h];

    // 1. Plot linear identity diagonal '.'
    for col in 0..w {
        let norm = (col as f64) / ((w - 1) as f64);
        let row = ((1.0 - norm) * ((h - 1) as f64)).round() as usize;
        grid[row.min(h - 1)][col] = '.';
    }

    // 2. Plot normalized raw response 'r'
    for &(fx, ry) in &lut.raw_response_norm {
        let col = (fx.clamp(0.0, 1.0) * ((w - 1) as f64)).round() as usize;
        let row = ((1.0 - ry.clamp(0.0, 1.0)) * ((h - 1) as f64)).round() as usize;
        grid[row.min(h - 1)][col.min(w - 1)] = 'r';
    }

    // 3. Plot final inverse LUT '#'
    for pt in &lut.points {
        let col = (pt.x.clamp(0.0, 1.0) * ((w - 1) as f64)).round() as usize;
        let row = ((1.0 - pt.y.clamp(0.0, 1.0)) * ((h - 1) as f64)).round() as usize;
        grid[row.min(h - 1)][col.min(w - 1)] = '#';
    }

    let mut out = String::with_capacity((w + 16) * (h + 5));
    let _ = writeln!(
        out,
        "  Legend: [#] Inverse LUT Output   [r] Raw Wheel Response   [.] Linear 1:1 Diagonal"
    );
    for (r_idx, row_chars) in grid.iter().enumerate() {
        let y_val = 1.0 - (r_idx as f64) / ((h - 1) as f64);
        let line_str: String = row_chars.iter().collect();
        let _ = writeln!(out, "  {y_val:4.2} |{line_str}|");
    }
    let _ = writeln!(out, "       +{}+", "-".repeat(w));
    let _ = writeln!(
        out,
        "        0.00{}0.50{}1.00",
        " ".repeat(w / 2 - 4),
        " ".repeat(w - (w / 2) - 4)
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lut_boundary_and_monotonicity_invariants() {
        // Synthetic wheel with a 14.5% deadzone and a non-monotonic noise spike at step 12
        let mut csv = String::from("force, startX, endX, deltaX, deltaXDeg\n0, 0, 0, 0, 0\n");
        let steps = 20;
        for s in 1..=steps {
            let force = (s * 10_000) / steps;
            let f_norm = (force as f64) / 10_000.0;
            let mut deg = if f_norm <= 0.145 {
                0.0
            } else {
                ((f_norm - 0.145) / (1.0 - 0.145)).powf(0.75) * 90.0
            };
            // Inject an artificial downward spike at s == 12
            if s == 12 {
                deg *= 0.65;
            }
            let delta_x = (deg * 22.0).round() as i32;
            let _ = writeln!(csv, "{force}, 0, {delta_x}, {delta_x}, {deg:.6}");
        }

        let samples = parse_wheelcheck_csv(&csv).expect("CSV should parse");
        let cfg = LutConfig::default();
        let res = generate_inverse_lut(&samples, &cfg).expect("LUT generation should succeed");

        assert_eq!(res.points.len(), 101);
        // Boundary invariants: lut[0] == (0.0, 0.0) and lut.last() == (1.0, 1.0)
        assert_eq!(res.points[0], LutPoint { x: 0.0, y: 0.0 });
        assert_eq!(res.points[100], LutPoint { x: 1.0, y: 1.0 });

        // Spike detection invariant
        assert!(res.corrected_spikes_count >= 1);

        // Strict monotonicity invariant: y[i] >= y[i-1] for all i
        for i in 1..res.points.len() {
            assert!(
                res.points[i].y >= res.points[i - 1].y,
                "Non-monotonic LUT at index {i}: {} < {}",
                res.points[i].y,
                res.points[i - 1].y
            );
        }
    }

    #[test]
    fn test_lut_deadzone_compensation_14_5_percent() {
        // Synthetic CSV with exact 14.5% deadzone (force = 1450 is last 0-movement sample)
        let mut samples = vec![
            StepLogSample {
                force: 0,
                start_x: 0,
                end_x: 0,
                delta_x: 0,
                delta_x_deg: 0.0,
            },
            StepLogSample {
                force: 1_000,
                start_x: 0,
                end_x: 0,
                delta_x: 0,
                delta_x_deg: 0.0,
            },
            StepLogSample {
                force: 1_450,
                start_x: 0,
                end_x: 0,
                delta_x: 0,
                delta_x_deg: 0.0,
            },
        ];
        for s in 1..=10 {
            let f = 1_450 + (s * (10_000 - 1_450)) / 10;
            let deg = (s as f64) * 10.0;
            samples.push(StepLogSample {
                force: f,
                start_x: 0,
                end_x: (deg * 20.0) as i32,
                delta_x: (deg * 20.0) as i32,
                delta_x_deg: deg,
            });
        }

        let cfg = LutConfig {
            output_steps: 100,
            smoothing_window: 3,
            deadzone_threshold_ratio: 0.005,
            center_preload_cap: None,
        };
        let res = generate_inverse_lut(&samples, &cfg).unwrap();

        assert!(
            (res.deadzone_floor_norm - 0.145).abs() < 1e-6,
            "Expected deadzone_floor_norm = 0.145, got {}",
            res.deadzone_floor_norm
        );
        // First non-zero LUT step (x = 0.01) must jump directly to >= 0.145 (~0.154)
        let first_nonzero = res.points[1];
        assert_eq!(first_nonzero.x, 0.01);
        assert!(
            first_nonzero.y >= 0.145 && first_nonzero.y <= 0.160,
            "Expected first non-zero LUT step to jump to ~0.145..0.160, got {}",
            first_nonzero.y
        );
    }

    #[test]
    fn test_lut_identity_invariant_for_linear_wheel() {
        // Perfectly linear wheel: deltaXDeg is strictly proportional to force with 0% deadzone
        let mut samples = Vec::new();
        for s in 0..=50 {
            let force = s * 200; // 0..=10000
            let deg = (s as f64) * 2.0; // 0.0..=100.0 deg
            samples.push(StepLogSample {
                force,
                start_x: 0,
                end_x: s * 100,
                delta_x: s * 100,
                delta_x_deg: deg,
            });
        }

        let cfg = LutConfig {
            output_steps: 100,
            smoothing_window: 5,
            deadzone_threshold_ratio: 0.005,
            center_preload_cap: None,
        };
        let res = generate_inverse_lut(&samples, &cfg).unwrap();

        assert_eq!(res.deadzone_floor_norm, 0.0);
        for pt in &res.points {
            assert!(
                (pt.y - pt.x).abs() <= 1e-3,
                "Identity invariant violated at x={}: y={}",
                pt.x,
                pt.y
            );
        }
    }
}
