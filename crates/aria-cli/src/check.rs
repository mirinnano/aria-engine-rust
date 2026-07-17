use std::path::Path;

use anyhow::Result;
use aria_core::Severity;

use crate::project::LoadedProject;

pub fn command(path: &Path, json: bool, release: bool) -> Result<u8> {
    let project = LoadedProject::load(path)?;
    let output = project.compile()?;
    if release {
        let assets = project.asset_inventory()?;
        project.validate_bundled_fonts(&assets, true)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&output.diagnostics)?);
    } else {
        for diagnostic in &output.diagnostics {
            eprintln!("{diagnostic}");
        }
        if let Some(program) = &output.program {
            println!(
                "checked {}: {} instructions, {} constants, {} warning(s)",
                project.manifest.game.id,
                program.instructions.len(),
                program.constants.len(),
                output
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.severity == Severity::Warning)
                    .count()
            );
        }
    }
    let unsupported_count = crate::release::unsupported_runtime_command_count(&output);
    let modern_language = crate::release::has_release_language(&output);
    if release && unsupported_count > 0 && !json {
        eprintln!(
            "error: release check rejected {unsupported_count} unsupported runtime command(s)"
        );
    }
    if release && !modern_language && !json {
        eprintln!(
            "error: release check requires structured 'aria 3.1;' source; run 'aria migrate' first"
        );
    }
    Ok(
        if output.has_errors() || (release && (unsupported_count > 0 || !modern_language)) {
            2
        } else {
            0
        },
    )
}
