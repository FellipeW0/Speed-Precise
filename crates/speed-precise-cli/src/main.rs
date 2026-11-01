//! `speed-precise-cli`: Command-line interface for DirectInput8 device inspection (`list`),
//! dual-handle Raw/Cooked telemetry (`monitor`), live calibration (`calibrate`),
//! `1000 Hz` `Step Log 2` linearity sweeps (`step-log2`), and inverse LUT generation (`generate-lut`, `auto-tune`).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use speed_precise_core::{
    generate_inverse_lut, inject_assetto_corsa_lut, read_wheelcheck_csv_file,
    render_ascii_lut_chart, run_step_log2_with_backend, write_lut_file, write_step_log2_csv,
    CoreError, CoreResult, DirectInputContext, FfbSession, LutConfig, StepLog2Config,
    VirtualWheelSimulator, DEFAULT_DEADZONE_THRESHOLD_RATIO, DEFAULT_LUT_STEPS,
    DEFAULT_SMOOTHING_WINDOW, DEFAULT_STEP_LOG2_DURATION_MS, DEFAULT_STEP_LOG2_MAX_ANGLE_DEG,
    DEFAULT_STEP_LOG2_MAX_FORCE, DEFAULT_STEP_LOG2_SETTLE_MS, DEFAULT_STEP_LOG2_STEPS,
    ENDSTOP_SAFETY_LIMIT_NORM,
};

#[derive(Parser, Debug)]
#[command(
    name = "speed-precise-cli",
    version,
    about = "Speed Precise CLI — DirectInput8 Telemetry, Live Calibration, Step Log 2 & Inverse LUT Auto-Tune"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Enumerates connected DirectInput8 controllers, axes, logical ranges, and live calibration
    List,

    /// Polls simultaneous Raw and Calibrated (Cooked) handles and renders live telemetry until Ctrl+C
    Monitor {
        /// Controller index from `list` (0-based)
        #[arg(short, long)]
        device: usize,

        /// Polling sleep interval in milliseconds (default: 10 ms = 100 Hz UI update)
        #[arg(long, default_value_t = 10)]
        interval_ms: u64,
    },

    /// Applies live hardware calibration via DIPROP_CALIBRATION (0x0B) and verifies round-trip readback
    Calibrate {
        /// Controller index from `list` (0-based)
        #[arg(short, long)]
        device: usize,

        /// Axis index (0, 1, ...) or DirectInput dwType ID (e.g. 258 or 0x00000102)
        #[arg(short, long, value_parser = parse_u32_decimal_or_hex)]
        axis: u32,

        /// Minimum raw calibration bound (lMin)
        #[arg(long, allow_hyphen_values = true)]
        min: i32,

        /// Center raw calibration bound (lCenter)
        #[arg(long, allow_hyphen_values = true)]
        center: i32,

        /// Maximum raw calibration bound (lMax)
        #[arg(long, allow_hyphen_values = true)]
        max: i32,

        /// Optional deadzone in DirectInput units (0..=10000, where 1000 = 10.0%)
        #[arg(long)]
        deadzone: Option<u32>,

        /// Optional saturation in DirectInput units (0..=10000, where 10000 = 100.0%)
        #[arg(long)]
        saturation: Option<u32>,
    },

    /// Executes the WheelCheck Step Log 2 Force Feedback linearity test and exports log2.csv
    StepLog2 {
        /// Controller index from `list` (default: 0; ignored when `--dry-run` is active)
        #[arg(short, long, default_value_t = 0)]
        device: usize,

        /// Number of discrete force steps (default: 50)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_STEPS)]
        steps: u32,

        /// Duration of each constant-force pulse in milliseconds (default: 300)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_DURATION_MS)]
        duration_ms: u64,

        /// Total steering lock-to-lock angle in degrees (default: 900.0)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_MAX_ANGLE_DEG)]
        max_angle: f64,

        /// Maximum DirectInput force magnitude in 1..=10000 (default: 10000; use e.g. 3500 for a gentle test)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_MAX_FORCE)]
        max_force: i32,

        /// Force direction multiplier (`1` = standard DirectInput, `-1` = inverted)
        #[arg(long, default_value_t = 1, allow_hyphen_values = true)]
        direction: i32,

        /// Output CSV path (default: log2.csv)
        #[arg(short, long, default_value = "log2.csv")]
        output: PathBuf,

        /// Run using the deterministic virtual wheel simulator without physical hardware
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Computes a smoothed inverse Force Feedback `.lut` table from a WheelCheck `log2.csv` file
    GenerateLut {
        /// Input WheelCheck Step Log 2 CSV file path
        #[arg(short, long)]
        input: PathBuf,

        /// Output `.lut` file path (default: my_wheel.lut)
        #[arg(short, long, default_value = "my_wheel.lut")]
        output: PathBuf,

        /// Number of uniform output steps in 0.00..=1.00 (default: 100 -> 101 lines)
        #[arg(long, default_value_t = DEFAULT_LUT_STEPS)]
        steps: usize,

        /// Moving-average smoothing window size (default: 5)
        #[arg(long, default_value_t = DEFAULT_SMOOTHING_WINDOW)]
        smoothing_window: usize,

        /// Automatically inject the generated `.lut` and activate it in Assetto Corsa `ff_post_process.ini`
        #[arg(long, default_value_t = false)]
        inject_ac: bool,

        /// Optional custom Assetto Corsa `cfg` directory (defaults to `%USERPROFILE%\Documents\Assetto Corsa\cfg`)
        #[arg(long)]
        ac_cfg_dir: Option<PathBuf>,
    },

    /// Unified All-in-One Pipeline: Step Log 2 -> In-Memory Inverse LUT Calculation -> `.lut` Export (+ AC Injection)
    AutoTune {
        /// Controller index from `list` (default: 0; ignored when `--dry-run` is active)
        #[arg(short, long, default_value_t = 0)]
        device: usize,

        /// Number of discrete force steps in Step Log 2 (default: 50)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_STEPS)]
        steps: u32,

        /// Duration of each constant-force pulse in milliseconds (default: 300)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_DURATION_MS)]
        duration_ms: u64,

        /// Total steering lock-to-lock angle in degrees (default: 900.0)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_MAX_ANGLE_DEG)]
        max_angle: f64,

        /// Maximum DirectInput force magnitude in 1..=10000 (default: 10000; use e.g. 3500 for a gentle test)
        #[arg(long, default_value_t = DEFAULT_STEP_LOG2_MAX_FORCE)]
        max_force: i32,

        /// Force direction multiplier (`1` = standard DirectInput, `-1` = inverted)
        #[arg(long, default_value_t = 1, allow_hyphen_values = true)]
        direction: i32,

        /// Output WheelCheck CSV path (default: log2.csv)
        #[arg(long, default_value = "log2.csv")]
        output_csv: PathBuf,

        /// Output inverse `.lut` file path (default: speed_precise.lut)
        #[arg(long, default_value = "speed_precise.lut")]
        output_lut: PathBuf,

        /// Moving-average smoothing window size for LUT generation (default: 5)
        #[arg(long, default_value_t = DEFAULT_SMOOTHING_WINDOW)]
        smoothing_window: usize,

        /// Automatically inject the generated `.lut` and activate it in Assetto Corsa `ff_post_process.ini`
        #[arg(long, default_value_t = false)]
        inject_ac: bool,

        /// Optional custom Assetto Corsa `cfg` directory (defaults to `%USERPROFILE%\Documents\Assetto Corsa\cfg`)
        #[arg(long)]
        ac_cfg_dir: Option<PathBuf>,

        /// Run using the deterministic virtual wheel simulator without physical hardware
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

fn parse_u32_decimal_or_hex(s: &str) -> Result<u32, String> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).map_err(|e| format!("Invalid hex axis ID `{s}`: {e}"))
    } else {
        trimmed
            .parse::<u32>()
            .map_err(|e| format!("Invalid axis ID `{s}`: {e}"))
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("[ERROR] {err}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> CoreResult<()> {
    match cli.command {
        Commands::List => {
            let ctx = DirectInputContext::new()?;
            cmd_list(&ctx)
        }
        Commands::Monitor {
            device,
            interval_ms,
        } => {
            let ctx = DirectInputContext::new()?;
            cmd_monitor(&ctx, device, interval_ms)
        }
        Commands::Calibrate {
            device,
            axis,
            min,
            center,
            max,
            deadzone,
            saturation,
        } => {
            let ctx = DirectInputContext::new()?;
            cmd_calibrate(&ctx, device, axis, min, center, max, deadzone, saturation)
        }
        Commands::StepLog2 {
            device,
            steps,
            duration_ms,
            max_angle,
            max_force,
            direction,
            output,
            dry_run,
        } => cmd_step_log2(
            device,
            steps,
            duration_ms,
            max_angle,
            max_force,
            direction,
            &output,
            dry_run,
        ),
        Commands::GenerateLut {
            input,
            output,
            steps,
            smoothing_window,
            inject_ac,
            ac_cfg_dir,
        } => cmd_generate_lut(
            &input,
            &output,
            steps,
            smoothing_window,
            inject_ac,
            ac_cfg_dir.as_deref(),
        ),
        Commands::AutoTune {
            device,
            steps,
            duration_ms,
            max_angle,
            max_force,
            direction,
            output_csv,
            output_lut,
            smoothing_window,
            inject_ac,
            ac_cfg_dir,
            dry_run,
        } => cmd_auto_tune(
            device,
            steps,
            duration_ms,
            max_angle,
            max_force,
            direction,
            &output_csv,
            &output_lut,
            smoothing_window,
            inject_ac,
            ac_cfg_dir.as_deref(),
            dry_run,
        ),
    }
}

fn cmd_list(ctx: &DirectInputContext) -> CoreResult<()> {
    let controllers = ctx.enumerate_controllers()?;
    println!("================================================================================");
    println!(
        " Speed Precise — DirectInput8 Connected Game Controllers ({})",
        controllers.len()
    );
    println!("================================================================================");

    if controllers.is_empty() {
        println!("No attached DirectInput8 game controllers (DI8DEVCLASS_GAMECTRL) detected.");
        return Ok(());
    }

    for dev in &controllers {
        println!(
            "[{idx}] {name} (Product: {prod})",
            idx = dev.index,
            name = dev.instance_name,
            prod = dev.product_name
        );
        println!("    VID:PID       : {:04X}:{:04X}", dev.vid, dev.pid);
        println!("    Instance GUID : {{{}}}", dev.guid_instance);
        println!("    Product GUID  : {{{}}}", dev.guid_product);
        println!(
            "    Capabilities  : FFB={} | Axes={} | Buttons={} | POVs={}",
            if dev.has_ffb { "YES" } else { "NO" },
            dev.axes.len(),
            dev.num_buttons,
            dev.num_povs
        );

        if dev.axes.is_empty() {
            println!("    Axes          : (none discovered)");
        } else {
            println!("    Discovered Axes:");
            for axis in &dev.axes {
                let reg_bytes = axis.calibration.to_registry_bytes();
                let hex_str = reg_bytes
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "      - Axis #{idx:<2} | dwType=0x{dwt:08X} ({dwt:<6}) | inst={inst:<2} | {std_lbl:<11} | \"{name}\"",
                    idx = axis.axis_index,
                    dwt = axis.dw_type,
                    inst = axis.instance_id,
                    std_lbl = axis.standard_label,
                    name = axis.name
                );
                println!(
                    "                 LogicalRange=[{lmin}, {lmax}] | OutputRange=[{omin}, {omax}]",
                    lmin = axis.logical_range.0,
                    lmax = axis.logical_range.1,
                    omin = axis.output_range.0,
                    omax = axis.output_range.1
                );
                println!(
                    "                 Calibration =[min: {cmin}, center: {cctr}, max: {cmax}] | DZ={dz} ({dz_pct:.1}%) | SAT={sat} ({sat_pct:.1}%)",
                    cmin = axis.calibration.min,
                    cctr = axis.calibration.center,
                    cmax = axis.calibration.max,
                    dz = axis.deadzone,
                    dz_pct = (axis.deadzone as f64) / 100.0,
                    sat = axis.saturation,
                    sat_pct = (axis.saturation as f64) / 100.0
                );
                println!("                 REG_BINARY  =[{hex_str}]");
            }
        }
        println!("--------------------------------------------------------------------------------");
    }

    Ok(())
}

fn cmd_monitor(ctx: &DirectInputContext, device_idx: usize, interval_ms: u64) -> CoreResult<()> {
    let mut dual = ctx.open_dual_handle(device_idx)?;

    let running = Arc::new(AtomicBool::new(true));
    let r_flag = Arc::clone(&running);
    let _ = ctrlc::set_handler(move || {
        r_flag.store(false, Ordering::SeqCst);
    });

    println!("================================================================================");
    println!(
        " Live Dual-Handle Telemetry — [{}] {} ({:04X}:{:04X})",
        dual.info.index, dual.info.instance_name, dual.info.vid, dual.info.pid
    );
    println!(" Mode: pRawDevice (DIPROPCALIBRATIONMODE_RAW=1) + pCookedDevice (COOKED=0)");
    println!(" Press Ctrl+C to stop monitoring.");
    println!("================================================================================");

    let sleep_dur = Duration::from_millis(interval_ms.max(1));
    let start_instant = Instant::now();
    let mut tick: u64 = 0;

    while running.load(Ordering::SeqCst) {
        let snapshots = dual.poll_telemetry()?;
        tick += 1;
        let elapsed = start_instant.elapsed().as_secs_f64();

        print!("\x1B[2J\x1B[H");
        println!(
            "Speed Precise Dual-Handle Monitor | Device [{}]: {} ({:04X}:{:04X}) | Elapsed: {:.1}s | Tick: {}",
            dual.info.index,
            dual.info.instance_name,
            dual.info.vid,
            dual.info.pid,
            elapsed,
            tick
        );
        println!("----------------------------------------------------------------------------------------");
        println!(
            "{:<4} {:<10} {:<16} {:>12} {:>12} {:>7}  {:<22}",
            "Idx", "dwType", "Axis Name", "Raw Encoder", "Calibrated", "Cooked%", "Bar [0%..100%]"
        );
        println!("----------------------------------------------------------------------------------------");

        for snap in &snapshots {
            let bar = render_progress_bar(snap.cooked_percent, 20);
            println!(
                "#{:<3} 0x{:08X} {:<16} {:>12} {:>12} {:>6.1}%  [{}]",
                snap.axis_index,
                snap.dw_type,
                truncate_str(&snap.name, 16),
                snap.raw_value,
                snap.cooked_value,
                snap.cooked_percent,
                bar
            );
        }
        println!("----------------------------------------------------------------------------------------");
        println!("Press Ctrl+C to exit gracefully (Unacquire + Release).");
        let _ = io::stdout().flush();

        thread::sleep(sleep_dur);
    }

    println!("\n[INFO] Ctrl+C received. Releasing DirectInput8 handles cleanly.");
    Ok(())
}

fn cmd_calibrate(
    ctx: &DirectInputContext,
    device_idx: usize,
    axis_selector: u32,
    min: i32,
    center: i32,
    max: i32,
    deadzone: Option<u32>,
    saturation: Option<u32>,
) -> CoreResult<()> {
    let mut dual = ctx.open_dual_handle(device_idx)?;
    let outcome =
        dual.apply_live_calibration(axis_selector, min, center, max, deadzone, saturation)?;

    let reg_bytes = outcome.readback_calibration.to_registry_bytes();
    let hex_str = reg_bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");

    println!("================================================================================");
    println!(" Live Calibration Applied & Verified (DIPROP_CALIBRATION = 0x0B)");
    println!("================================================================================");
    println!(
        " Device          : [{}] {} (VID:PID {:04X}:{:04X})",
        device_idx, outcome.device_name, outcome.vid, outcome.pid
    );
    println!(
        " Target Axis     : #{} (dwType=0x{:08X} / {}) \"{}\"",
        outcome.axis_index, outcome.axis_dw_type, outcome.axis_dw_type, outcome.axis_name
    );
    println!(" Safety Backup   : {}", outcome.backup_path.display());
    println!(
        " Previous Calib  : [min: {}, center: {}, max: {}]",
        outcome.previous_calibration.min,
        outcome.previous_calibration.center,
        outcome.previous_calibration.max
    );
    println!(
        " Readback Calib  : [min: {}, center: {}, max: {}]",
        outcome.readback_calibration.min,
        outcome.readback_calibration.center,
        outcome.readback_calibration.max
    );
    println!(
        " Readback DZ/SAT : Deadzone={} ({:.1}%) | Saturation={} ({:.1}%)",
        outcome.readback_deadzone,
        (outcome.readback_deadzone as f64) / 100.0,
        outcome.readback_saturation,
        (outcome.readback_saturation as f64) / 100.0
    );
    println!(" Registry Binary : [{hex_str}]");
    println!("================================================================================");

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_step_log2(
    device_idx: usize,
    steps: u32,
    duration_ms: u64,
    max_angle_deg: f64,
    max_force: i32,
    direction: i32,
    output_path: &Path,
    dry_run: bool,
) -> CoreResult<()> {
    let samples = execute_step_log2_run(
        device_idx,
        steps,
        duration_ms,
        max_angle_deg,
        max_force,
        direction,
        output_path,
        dry_run,
    )?;
    write_step_log2_csv(output_path, &samples)?;

    println!("--------------------------------------------------------------------------------");
    println!(
        " [SUCCESS] Step Log 2 completed ({} rows written to `{}`).",
        samples.len(),
        output_path.display()
    );
    println!("================================================================================");

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_step_log2_run(
    device_idx: usize,
    steps: u32,
    duration_ms: u64,
    max_angle_deg: f64,
    max_force: i32,
    direction: i32,
    output_path: &Path,
    dry_run: bool,
) -> CoreResult<Vec<speed_precise_core::StepLog2Sample>> {
    let config = StepLog2Config {
        steps,
        duration_ms,
        settle_ms: DEFAULT_STEP_LOG2_SETTLE_MS,
        max_angle_deg,
        max_force,
        direction,
    };
    config.validate()?;

    let abort_flag = Arc::new(AtomicBool::new(false));
    let abort_clone = Arc::clone(&abort_flag);
    let _ = ctrlc::set_handler(move || {
        abort_clone.store(true, Ordering::SeqCst);
    });

    println!("================================================================================");
    println!(" Speed Precise — WheelCheck Step Log 2 Force Feedback Linearity Test");
    println!("================================================================================");
    println!(
        " Parameters      : Steps={} | Pulse={} ms | Settle={} ms | MaxAngle={:.1} deg | MaxForce={} | Dir={}",
        config.steps, config.duration_ms, config.settle_ms, config.max_angle_deg, config.max_force, config.direction
    );
    println!(" Output CSV      : {}", output_path.display());
    println!(
        " Execution Mode  : {}",
        if dry_run {
            "VIRTUAL WHEEL SIMULATOR (--dry-run, 1000 Hz, 14.5% deadzone + log response)"
        } else {
            "PHYSICAL DIRECTINPUT8 HARDWARE (1000 Hz High-Res Timer | DISCL_EXCLUSIVE | GUID_ConstantForce)"
        }
    );
    println!(
        " Safety Cutoff   : Ctrl+C or |X_norm| >= {} (85% Soft-Lock + Anti-Runaway Guard + RAII Drop)",
        ENDSTOP_SAFETY_LIMIT_NORM
    );
    println!("--------------------------------------------------------------------------------");

    let run_start = Instant::now();
    let samples = if dry_run {
        let mut sim = VirtualWheelSimulator::new();
        run_step_log2_with_backend(&mut sim, &config, &abort_flag, |prog| {
            println!(
                "  [Step {:>3}/{:>3}] Force={:>5} | startX={:>6} | endX={:>6} | deltaX={:>6} | deltaXDeg={:>9.4} deg{}",
                prog.step_index,
                prog.total_steps,
                prog.sample.force,
                prog.sample.start_x,
                prog.sample.end_x,
                prog.sample.delta_x,
                prog.sample.delta_x_deg,
                if prog.early_cutoff { " [ENDSTOP CUTOFF]" } else { "" }
            );
        })?
    } else {
        let ctx = DirectInputContext::new()?;
        let controllers = ctx.enumerate_controllers()?;
        let total = controllers.len();
        let info = controllers
            .into_iter()
            .nth(device_idx)
            .ok_or(CoreError::DeviceIndexOutOfRange {
                index: device_idx,
                total,
            })?;

        println!(
            " Target Device   : [{}] {} ({:04X}:{:04X})",
            info.index, info.instance_name, info.vid, info.pid
        );
        println!("--------------------------------------------------------------------------------");

        let mut session = FfbSession::open(&ctx, info)?;
        run_step_log2_with_backend(&mut session, &config, &abort_flag, |prog| {
            println!(
                "  [Step {:>3}/{:>3}] Force={:>5} | startX={:>6} | endX={:>6} | deltaX={:>6} | deltaXDeg={:>9.4} deg{}",
                prog.step_index,
                prog.total_steps,
                prog.sample.force,
                prog.sample.start_x,
                prog.sample.end_x,
                prog.sample.delta_x,
                prog.sample.delta_x_deg,
                if prog.early_cutoff { " [ENDSTOP CUTOFF]" } else { "" }
            );
        })?
    };

    let elapsed_s = run_start.elapsed().as_secs_f64();
    println!(
        " Wall-Clock Time : {:.2}s total ({:.3}s/step avg)",
        elapsed_s,
        elapsed_s / (config.steps.max(1) as f64)
    );
    Ok(samples)
}

fn cmd_generate_lut(
    input_csv: &Path,
    output_lut: &Path,
    steps: usize,
    smoothing_window: usize,
    inject_ac: bool,
    ac_cfg_dir: Option<&Path>,
) -> CoreResult<()> {
    let samples = read_wheelcheck_csv_file(input_csv)?;
    process_and_export_lut(
        &samples,
        output_lut,
        steps,
        smoothing_window,
        inject_ac,
        ac_cfg_dir,
    )
}

#[allow(clippy::too_many_arguments)]
fn cmd_auto_tune(
    device_idx: usize,
    steps: u32,
    duration_ms: u64,
    max_angle_deg: f64,
    max_force: i32,
    direction: i32,
    output_csv: &Path,
    output_lut: &Path,
    smoothing_window: usize,
    inject_ac: bool,
    ac_cfg_dir: Option<&Path>,
    dry_run: bool,
) -> CoreResult<()> {
    let samples = execute_step_log2_run(
        device_idx,
        steps,
        duration_ms,
        max_angle_deg,
        max_force,
        direction,
        output_csv,
        dry_run,
    )?;
    write_step_log2_csv(output_csv, &samples)?;
    println!("--------------------------------------------------------------------------------");
    println!(
        " [STAGE 1/2] Step Log 2 CSV saved to `{}` ({} samples).",
        output_csv.display(),
        samples.len()
    );

    process_and_export_lut(
        &samples,
        output_lut,
        DEFAULT_LUT_STEPS,
        smoothing_window,
        inject_ac,
        ac_cfg_dir,
    )
}

fn process_and_export_lut(
    samples: &[speed_precise_core::StepLogSample],
    output_lut: &Path,
    steps: usize,
    smoothing_window: usize,
    inject_ac: bool,
    ac_cfg_dir: Option<&Path>,
) -> CoreResult<()> {
    let lut_cfg = LutConfig {
        output_steps: steps,
        smoothing_window,
        deadzone_threshold_ratio: DEFAULT_DEADZONE_THRESHOLD_RATIO,
        center_preload_cap: None,
    };
    let lut_res = generate_inverse_lut(samples, &lut_cfg)?;
    write_lut_file(output_lut, &lut_res)?;

    let first_nonzero_y = lut_res.points.get(1).map(|p| p.y).unwrap_or(0.0);

    println!("================================================================================");
    println!(" Speed Precise — Inverse Force Feedback LUT Analysis & Export");
    println!("================================================================================");
    println!(" Input Samples        : {}", samples.len());
    println!(
        " Peak Displacement    : {:.2} deg (Max Force: {})",
        lut_res.max_displacement_deg, lut_res.max_force
    );
    println!(
        " Detected Deadzone    : {:.1}% (Normalized Floor: {:.3}, Preload Step[1]: {:.3})",
        lut_res.deadzone_percent, lut_res.deadzone_floor_norm, first_nonzero_y
    );
    println!(
        " Corrected Spikes     : {} sample(s) smoothed/monotonicized",
        lut_res.corrected_spikes_count
    );
    println!(
        " Output LUT File      : {} ({} points, smoothing window={})",
        output_lut.display(),
        lut_res.points.len(),
        smoothing_window
    );
    println!("--------------------------------------------------------------------------------");
    print!("{}", render_ascii_lut_chart(&lut_res, 56, 16));
    println!("--------------------------------------------------------------------------------");

    if inject_ac {
        let lut_filename = output_lut
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("speed_precise.lut");
        let ac = inject_assetto_corsa_lut(&lut_res, lut_filename, ac_cfg_dir)?;
        println!(" [ASSETTO CORSA INJECTION COMPLETED]");
        println!("   Config Dir   : {}", ac.cfg_dir.display());
        println!("   Injected LUT : {}", ac.lut_path.display());
        println!("   Updated INI  : {} (TYPE=LUT, ENABLED=1, CURVE={})", ac.ini_path.display(), ac.lut_filename);
        if let Some(bak) = ac.backup_ini_path {
            println!("   INI Backup   : {}", bak.display());
        }
        println!("--------------------------------------------------------------------------------");
    }

    println!(" [SUCCESS] Inverse LUT pipeline completed.");
    println!("================================================================================");
    Ok(())
}

fn render_progress_bar(percent: f64, width: usize) -> String {
    let clamped = percent.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width);
    for i in 0..width {
        if i < filled {
            s.push('#');
        } else {
            s.push('-');
        }
    }
    s
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_u32_decimal_or_hex() {
        assert_eq!(parse_u32_decimal_or_hex("0").unwrap(), 0);
        assert_eq!(parse_u32_decimal_or_hex("258").unwrap(), 258);
        assert_eq!(parse_u32_decimal_or_hex("0x00000102").unwrap(), 0x0000_0102);
        assert_eq!(parse_u32_decimal_or_hex("0X102").unwrap(), 0x102);
        assert!(parse_u32_decimal_or_hex("invalid").is_err());
    }

    #[test]
    fn test_cli_negative_min_center_max_parsing() {
        let cli = Cli::try_parse_from([
            "speed-precise-cli",
            "calibrate",
            "--device",
            "0",
            "--axis",
            "0x00000102",
            "--min",
            "-32768",
            "--center",
            "0",
            "--max",
            "32767",
            "--deadzone",
            "250",
            "--saturation",
            "9800",
        ])
        .expect("CLI should parse negative calibration values and hex axis IDs");

        match cli.command {
            Commands::Calibrate {
                device,
                axis,
                min,
                center,
                max,
                deadzone,
                saturation,
            } => {
                assert_eq!(device, 0);
                assert_eq!(axis, 0x0000_0102);
                assert_eq!(min, -32768);
                assert_eq!(center, 0);
                assert_eq!(max, 32767);
                assert_eq!(deadzone, Some(250));
                assert_eq!(saturation, Some(9800));
            }
            _ => panic!("Expected Calibrate subcommand"),
        }
    }

    #[test]
    fn test_cli_step_log2_arguments_parsing() {
        let cli = Cli::try_parse_from([
            "speed-precise-cli",
            "step-log2",
            "--device",
            "1",
            "--steps",
            "25",
            "--duration-ms",
            "300",
            "--max-angle",
            "1080.0",
            "--max-force",
            "3500",
            "--direction",
            "1",
            "--output",
            "test_log2.csv",
            "--dry-run",
        ])
        .expect("CLI should parse step-log2 arguments");

        match cli.command {
            Commands::StepLog2 {
                device,
                steps,
                duration_ms,
                max_angle,
                max_force,
                direction,
                output,
                dry_run,
            } => {
                assert_eq!(device, 1);
                assert_eq!(steps, 25);
                assert_eq!(duration_ms, 300);
                assert!((max_angle - 1080.0).abs() < 1e-6);
                assert_eq!(max_force, 3500);
                assert_eq!(direction, 1);
                assert_eq!(output, PathBuf::from("test_log2.csv"));
                assert!(dry_run);
            }
            _ => panic!("Expected StepLog2 subcommand"),
        }
    }

    #[test]
    fn test_cli_generate_lut_and_auto_tune_parsing() {
        let gen_cli = Cli::try_parse_from([
            "speed-precise-cli",
            "generate-lut",
            "--input",
            "log2.csv",
            "--output",
            "g29.lut",
            "--steps",
            "100",
            "--smoothing-window",
            "5",
            "--inject-ac",
            "--ac-cfg-dir",
            "C:\\temp\\ac_cfg",
        ])
        .expect("CLI should parse generate-lut arguments");

        match gen_cli.command {
            Commands::GenerateLut {
                input,
                output,
                steps,
                smoothing_window,
                inject_ac,
                ac_cfg_dir,
            } => {
                assert_eq!(input, PathBuf::from("log2.csv"));
                assert_eq!(output, PathBuf::from("g29.lut"));
                assert_eq!(steps, 100);
                assert_eq!(smoothing_window, 5);
                assert!(inject_ac);
                assert_eq!(ac_cfg_dir, Some(PathBuf::from("C:\\temp\\ac_cfg")));
            }
            _ => panic!("Expected GenerateLut subcommand"),
        }

        let auto_cli = Cli::try_parse_from([
            "speed-precise-cli",
            "auto-tune",
            "--steps",
            "20",
            "--output-csv",
            "auto_log2.csv",
            "--output-lut",
            "auto.lut",
            "--smoothing-window",
            "5",
            "--inject-ac",
            "--dry-run",
        ])
        .expect("CLI should parse auto-tune arguments");

        match auto_cli.command {
            Commands::AutoTune {
                steps,
                output_csv,
                output_lut,
                smoothing_window,
                inject_ac,
                dry_run,
                ..
            } => {
                assert_eq!(steps, 20);
                assert_eq!(output_csv, PathBuf::from("auto_log2.csv"));
                assert_eq!(output_lut, PathBuf::from("auto.lut"));
                assert_eq!(smoothing_window, 5);
                assert!(inject_ac);
                assert!(dry_run);
            }
            _ => panic!("Expected AutoTune subcommand"),
        }
    }
}
