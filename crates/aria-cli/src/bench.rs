//! `aria bench`: drives a project's VM with scripted input and reports hot-loop
//! timings. This is the baseline/regression harness for engine performance
//! work; it intentionally uses no external benchmarking crates so the numbers
//! come from the same `Vm::step` path production runtimes use.

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use aria_core::{InputSnapshot, UiIntent, Vm};

use crate::run::load_runtime_project;

#[derive(Debug, serde::Serialize)]
struct BenchReport {
    project: String,
    steps_requested: u64,
    steps_executed: u64,
    halted: bool,
    wall_time_ms: f64,
    steps_per_sec: f64,
    avg_step_us: f64,
    peak_rss_bytes: u64,
}

pub fn command(path: &Path, steps: u64, json: bool) -> Result<u8> {
    let project = load_runtime_project(path)?;
    let mut vm = Vm::new(project.program, project.logical_size)?;

    let mut sequence = 0u64;
    let mut executed = 0u64;
    let mut step_nanos = 0u128;

    sequence += 1;
    let start = Instant::now();
    let mut output = vm.step(&InputSnapshot::idle(sequence, 16))?;
    step_nanos += start.elapsed().as_nanos();
    executed += 1;

    // Scripted policy: advance the dialogue every frame, and take the first
    // choice whenever one is presented. No waits, no sleeps: the loop measures
    // raw VM throughput, not wall-clock pacing.
    while !output.halted && executed < steps {
        sequence += 1;
        let mut input = InputSnapshot::idle(sequence, 16);
        let id = output
            .view
            .choices
            .first()
            .map(|choice| choice.id.clone())
            .unwrap_or_else(|| "dialogue.advance".to_owned());
        input.intents.push(UiIntent::Activate { id });
        let start = Instant::now();
        output = vm.step(&input)?;
        step_nanos += start.elapsed().as_nanos();
        executed += 1;
    }

    let step_secs = step_nanos as f64 / 1e9;
    let report = BenchReport {
        project: path.display().to_string(),
        steps_requested: steps,
        steps_executed: executed,
        halted: output.halted,
        wall_time_ms: step_secs * 1e3,
        steps_per_sec: if step_secs > 0.0 {
            executed as f64 / step_secs
        } else {
            0.0
        },
        avg_step_us: if executed > 0 {
            step_nanos as f64 / 1e3 / executed as f64
        } else {
            0.0
        },
        peak_rss_bytes: peak_rss_bytes(),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("aria bench: {}", report.project);
        println!(
            "steps: {} executed of {} requested (halted: {})",
            report.steps_executed, report.steps_requested, report.halted
        );
        println!(
            "wall: {:.3} s ({:.0} steps/s, avg {:.1} us/step)",
            report.wall_time_ms / 1e3,
            report.steps_per_sec,
            report.avg_step_us
        );
        println!(
            "peak RSS (VmHWM): {:.1} MiB",
            report.peak_rss_bytes as f64 / (1024.0 * 1024.0)
        );
    }
    Ok(0)
}

/// Peak resident set size of this process. Linux exposes the high-water mark
/// via /proc; other platforms report 0 rather than guessing.
fn peak_rss_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("VmHWM:")
                    && let Some(kb) = rest
                        .trim()
                        .strip_suffix("kB")
                        .and_then(|value| value.trim().parse::<u64>().ok())
                {
                    return kb * 1024;
                }
            }
        }
    }
    0
}
