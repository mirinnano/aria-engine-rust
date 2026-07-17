use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use aria_core::pak::PakArchive;
use aria_core::protocol::{DrawCommand, LogicalSize, RuntimeCommand, StepOutput, UiRole};
use aria_core::{CompiledProgram, InputAction, InputSnapshot, SaveEnvelopeV3, Vm, VmSnapshot};
use aria_native::{AtomicSaveStore, ReplayRunner, ReplayTape};
use aria_protection::PakPackage;
use fontdb::Database as FontDatabase;

use crate::build::{
    BuildManifest, BuildProfile, BuildTarget, BundleManifest, bundle_content_root, resolve_pak_keys,
};
use crate::package_runtime::native_player_filename;
use crate::project::{AssetInventory, LoadedProject};

#[derive(Debug, Clone)]
#[cfg_attr(
    any(not(feature = "desktop-player"), target_arch = "wasm32"),
    allow(dead_code)
)]
pub(crate) enum RuntimeAssetSource {
    ProjectRoot { assets: AssetInventory },
    Package { profile: BuildProfile },
}

#[derive(Debug)]
pub(crate) struct RuntimeProject {
    pub(crate) root: PathBuf,
    pub(crate) program: CompiledProgram,
    pub(crate) logical_size: LogicalSize,
    pub(crate) save_namespace: String,
    #[cfg_attr(
        any(not(feature = "desktop-player"), target_arch = "wasm32"),
        allow(dead_code)
    )]
    pub(crate) font_assets: Vec<String>,
    #[cfg_attr(
        any(not(feature = "desktop-player"), target_arch = "wasm32"),
        allow(dead_code)
    )]
    pub(crate) title: String,
    #[cfg_attr(
        any(not(feature = "desktop-player"), target_arch = "wasm32"),
        allow(dead_code)
    )]
    pub(crate) asset_source: RuntimeAssetSource,
}

pub fn command(path: &Path, headless: bool, replay: Option<&Path>, max_frames: u64) -> Result<u8> {
    let project = load_runtime_project(path)?;
    if let Some(replay) = replay {
        let tape: ReplayTape = serde_json::from_slice(
            &fs::read(replay).with_context(|| format!("cannot read {}", replay.display()))?,
        )?;
        let (result, _) = ReplayRunner.run(project.program, project.logical_size, &tape)?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(0);
    }

    let mut vm = Vm::new(project.program, project.logical_size)?;
    let save_store = AtomicSaveStore::new(project.root.join("saves-v3"), project.save_namespace)?;
    let interactive = !headless && io::stdin().is_terminal() && io::stdout().is_terminal();
    let mut sequence = 1;
    let mut output = vm.step(&InputSnapshot::idle(sequence, 16))?;
    process_runtime_commands(&mut vm, &save_store, &output.runtime)?;

    if !interactive {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(0);
    }

    let mut frames = 1;
    while !output.halted && frames < max_frames {
        print_terminal_frame(&output);
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let selected = line.trim().parse::<usize>().ok();
        let focused = output
            .ui
            .nodes
            .values()
            .filter(|node| node.role == UiRole::Button)
            .position(|node| node.focused)
            .unwrap_or(0);

        if let Some(selected) = selected.filter(|selected| *selected > 0) {
            let target = selected - 1;
            let action = if target >= focused {
                InputAction::NavigateDown
            } else {
                InputAction::NavigateUp
            };
            for _ in 0..target.abs_diff(focused) {
                sequence += 1;
                drop(vm.step(&InputSnapshot::pressed(sequence, 16, action))?);
                frames += 1;
            }
            sequence += 1;
            output = vm.step(&InputSnapshot::pressed(sequence, 16, InputAction::Confirm))?;
        } else {
            sequence += 1;
            output = vm.step(&InputSnapshot::pressed(sequence, 16, InputAction::Advance))?;
        }
        frames += 1;
        process_runtime_commands(&mut vm, &save_store, &output.runtime)?;
    }
    if frames >= max_frames && !output.halted {
        bail!("run exceeded --max-frames {max_frames}");
    }
    Ok(0)
}

pub(crate) fn load_runtime_project(path: &Path) -> Result<RuntimeProject> {
    let root = if path.is_dir() {
        path.canonicalize()?
    } else {
        path.parent()
            .context("runtime path has no parent")?
            .canonicalize()?
    };
    let packaged_program = root.join("game.ariac");
    let packaged_bundle = root.join("bundle.aria.json");
    if packaged_program.is_file() && packaged_bundle.is_file() {
        let bundle: BundleManifest = serde_json::from_slice(&fs::read(&packaged_bundle)?)?;
        validate_bundle(&bundle)?;
        let ariac = fs::read(&packaged_program)?;
        if blake3::hash(&ariac).to_hex().as_str() != bundle.ariac_blake3 {
            bail!("game.ariac does not match bundle.aria.json");
        }
        if u64::try_from(ariac.len())? != bundle.ariac_size {
            bail!("game.ariac size does not match bundle.aria.json");
        }
        let program = CompiledProgram::decode(&ariac)?;
        if program.game_id != bundle.game_id
            || program.language_version.major != bundle.language_major
            || program.language_version.minor != bundle.language_minor
        {
            bail!("game.ariac metadata does not match bundle.aria.json");
        }
        let pak_path = root.join("game.ariapak");
        let pak =
            fs::read(&pak_path).with_context(|| format!("cannot read {}", pak_path.display()))?;
        if blake3::hash(&pak).to_hex().as_str() != bundle.pak_blake3 {
            bail!("game.ariapak does not match bundle.aria.json");
        }
        if u64::try_from(pak.len())? != bundle.pak_size {
            bail!("game.ariapak size does not match bundle.aria.json");
        }
        let package_profile = bundle.pak_profile;
        match package_profile {
            BuildProfile::Dev => {
                let archive = PakArchive::open(&pak)?;
                if archive.game_id() != bundle.game_id
                    || archive.content_root_hex() != bundle.pak_content_root_blake3
                {
                    bail!("game.ariapak metadata does not match bundle.aria.json");
                }
                validate_packaged_fonts(&bundle, |path| {
                    archive.read(path).map_err(|error| anyhow::anyhow!(error))
                })?;
            }
            BuildProfile::Signed | BuildProfile::Protected => {
                let keys = resolve_pak_keys(package_profile, None, None)?;
                let mut provider = aria_protection::StaticPakKeyProvider::new();
                if let Some(key) = keys.signing.as_ref() {
                    provider = provider.with_signing_key(key);
                }
                if let Some(key) = keys.encryption.as_ref() {
                    provider = provider.with_encryption_key(key);
                }
                let package = PakPackage::open(&pak, Some(&provider))?;
                let manifest = package.manifest();
                if manifest.game_id != bundle.game_id
                    || manifest.pack_id != bundle.pack_id
                    || package.content_root() != bundle.pak_content_root_blake3
                {
                    bail!("game.ariapak manifest does not match bundle.aria.json");
                }
                validate_packaged_fonts(&bundle, |path| {
                    package.read(path).map_err(|error| anyhow::anyhow!(error))
                })?;
            }
        }
        let wrapper = root.join("build-manifest.json");
        if wrapper.is_file() {
            let manifest: BuildManifest = serde_json::from_slice(&fs::read(&wrapper)?)?;
            let bundle_bytes = fs::read(&packaged_bundle)?;
            if manifest.schema_version != 5
                || manifest.engine_version != bundle.engine_version
                || manifest.bundle_blake3 != blake3::hash(&bundle_bytes).to_hex().to_string()
            {
                bail!("build-manifest.json does not match bundle.aria.json");
            }
            validate_wrapper_files(&root, &manifest)?;
        }
        return Ok(RuntimeProject {
            root,
            program,
            logical_size: LogicalSize {
                width: bundle.logical_width,
                height: bundle.logical_height,
            },
            save_namespace: bundle.save_namespace.clone(),
            font_assets: bundle.font_assets.clone(),
            title: bundle.game_title.clone(),
            asset_source: RuntimeAssetSource::Package {
                profile: bundle.pak_profile,
            },
        });
    }

    let project = LoadedProject::load(path)?;
    let compiled = project.compile()?;
    for diagnostic in &compiled.diagnostics {
        eprintln!("{diagnostic}");
    }
    if compiled.has_errors() {
        bail!("project has compiler errors");
    }
    let assets = project.asset_inventory()?;
    Ok(RuntimeProject {
        root: project.root,
        program: compiled.program.context("compiler produced no program")?,
        logical_size: LogicalSize {
            width: project.manifest.runtime.logical_width,
            height: project.manifest.runtime.logical_height,
        },
        save_namespace: project.manifest.runtime.save_namespace,
        font_assets: project.manifest.runtime.fonts,
        title: project.manifest.game.title,
        asset_source: RuntimeAssetSource::ProjectRoot { assets },
    })
}

fn validate_wrapper_files(root: &Path, manifest: &BuildManifest) -> Result<()> {
    let native_player = match manifest.target {
        BuildTarget::Web => None,
        target => Some(root.join(native_player_filename(target))),
    };
    let player_present = native_player.as_ref().is_some_and(|path| path.is_file());
    if player_present != manifest.player_binary_included {
        bail!(
            "build-manifest.json Player inclusion does not match the package files for {}",
            manifest.target.as_str()
        );
    }
    if manifest.target == BuildTarget::Web && manifest.player_binary_included {
        bail!("Web build-manifest.json cannot include a native Player");
    }
    if manifest.target != BuildTarget::Web && manifest.web_runtime_package_required {
        bail!("non-Web build-manifest.json cannot require a Web runtime");
    }
    if manifest.target == BuildTarget::Web
        && !manifest.web_runtime_package_required
        && (!root.join("pkg/aria_web.js").is_file() || !root.join("pkg/aria_web_bg.wasm").is_file())
    {
        bail!("Web build-manifest.json claims a runtime package that is missing");
    }
    Ok(())
}

fn validate_packaged_fonts(
    bundle: &BundleManifest,
    mut read_asset: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<()> {
    for logical_path in &bundle.font_assets {
        let bytes = read_asset(logical_path)
            .with_context(|| format!("cannot read bundled font '{logical_path}'"))?;
        let mut database = FontDatabase::new();
        database.load_font_data(bytes);
        if database.faces().next().is_none() {
            bail!("bundled font '{logical_path}' is not a readable OpenType/TrueType font");
        }
    }
    Ok(())
}

fn validate_bundle(bundle: &BundleManifest) -> Result<()> {
    if bundle.schema_version != 5 {
        bail!(
            "unsupported bundle.aria.json schema {}",
            bundle.schema_version
        );
    }
    if bundle.vm_abi_version != aria_core::bytecode::ARIAC_VM_ABI_VERSION {
        bail!(
            "bundle.aria.json requires unsupported VM ABI {}",
            bundle.vm_abi_version
        );
    }
    if bundle.game_id.trim().is_empty()
        || bundle.save_namespace.trim().is_empty()
        || bundle.logical_width == 0
        || bundle.logical_height == 0
    {
        bail!("bundle.aria.json has invalid game/runtime metadata");
    }
    if bundle.content_root_blake3 != bundle_content_root(bundle) {
        bail!("bundle.aria.json content root does not match its metadata");
    }
    let mut font_paths = BTreeSet::new();
    let mut portable_font_paths = BTreeSet::new();
    for font in &bundle.font_assets {
        let canonical = aria_core::compiler::normalize_logical_path(font)
            .map_err(|error| anyhow::anyhow!("bundle font asset '{font}' is invalid: {error}"))?;
        if canonical != *font {
            bail!(
                "bundle font asset '{font}' is not a canonical NFC '/' logical path; use '{canonical}'"
            );
        }
        if !font_paths.insert(font) {
            bail!("bundle repeats font asset '{font}'");
        }
        let portable = aria_core::compiler::portable_path_key(font)
            .map_err(|error| anyhow::anyhow!("bundle font asset '{font}' is invalid: {error}"))?;
        if !portable_font_paths.insert(portable) {
            bail!("bundle font assets collide on a case-insensitive filesystem: '{font}'");
        }
    }
    Ok(())
}

fn process_runtime_commands(
    vm: &mut Vm,
    store: &AtomicSaveStore,
    commands: &[RuntimeCommand],
) -> Result<()> {
    let mut warned = BTreeSet::new();
    for command in commands {
        match command {
            RuntimeCommand::Save { slot } => {
                let snapshot = vm.snapshot();
                let envelope = SaveEnvelopeV3::new(
                    snapshot.game_id.clone(),
                    aria_core::ENGINE_VERSION,
                    now_unix_ms(),
                    &snapshot,
                )?;
                store.save(*slot, &envelope)?;
            }
            RuntimeCommand::Load { slot } => {
                if let Some(loaded) = store.load(*slot)? {
                    let snapshot: VmSnapshot = loaded.envelope.payload_as()?;
                    loaded.envelope.validate_for_game(&snapshot.game_id)?;
                    vm.restore(snapshot)?;
                    if loaded.recovered_from_previous {
                        eprintln!(
                            "warning: recovered save slot {slot} from the previous generation"
                        );
                    }
                }
            }
            RuntimeCommand::Unsupported { name, .. } if warned.insert(name.clone()) => {
                eprintln!("warning: runtime skipped vertical-slice host command '{name}'");
            }
            RuntimeCommand::Quit => break,
            RuntimeCommand::OpenMenu | RuntimeCommand::Unsupported { .. } => {}
        }
    }
    Ok(())
}

fn print_terminal_frame(output: &StepOutput) {
    if let Some(DrawCommand::Text { text, speaker, .. }) =
        output.render.commands.iter().find(
            |command| matches!(command, DrawCommand::Text { id, .. } if id == "vn.textbox.text"),
        )
    {
        if let Some(speaker) = speaker {
            println!("{speaker}: {text}");
        } else {
            println!("{text}");
        }
    }
    let choices = output
        .ui
        .nodes
        .values()
        .filter(|node| node.role == UiRole::Button)
        .collect::<Vec<_>>();
    for (index, choice) in choices.iter().enumerate() {
        println!(
            "{} {} {}",
            if choice.focused { ">" } else { " " },
            index + 1,
            choice.label
        );
    }
    println!(
        "[Enter: advance{}]",
        if choices.is_empty() {
            ""
        } else {
            ", number: choose"
        }
    );
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{BuildTarget, build_project};

    fn project(root: &Path) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("aria.toml"),
            "schema = 3\n\
             [game]\n\
             id = \"jp.example.package-integrity\"\n\
             version = \"3.0.0\"\n\
             title = \"Package integrity\"\n\
             [runtime]\n\
             entry = \"scripts/main.aria\"\n\
             logical_width = 640\n\
             logical_height = 360\n\
             asset_roots = [\"assets\"]\n\
             fonts = []\n\
             save_namespace = \"package-integrity\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.aria"),
            "# aria-version: 3.0\nstrict on\nbg \"#111820\", 0\nend\n",
        )
        .unwrap();
        fs::write(root.join("assets/placeholder.txt"), "asset").unwrap();
    }

    #[test]
    fn packaged_runtime_rejects_a_bytecode_file_that_no_longer_matches_manifest() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let output = temp.path().join("package");
        build_project(temp.path(), BuildTarget::LinuxX64, Some(&output)).unwrap();
        assert!(matches!(
            load_runtime_project(&output).unwrap().asset_source,
            RuntimeAssetSource::Package { .. }
        ));

        fs::write(output.join("game.ariac"), b"corrupt").unwrap();
        let error = load_runtime_project(&output).unwrap_err();
        assert!(error.to_string().contains("game.ariac does not match"));
    }

    #[test]
    fn packaged_runtime_rejects_a_font_manifest_entry_missing_from_the_pak() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let output = temp.path().join("package");
        build_project(temp.path(), BuildTarget::LinuxX64, Some(&output)).unwrap();

        let manifest_path = output.join("bundle.aria.json");
        let mut bundle: BundleManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        bundle.font_assets = vec!["assets/fonts/missing.ttf".to_owned()];
        bundle.content_root_blake3 = bundle_content_root(&bundle);
        fs::write(&manifest_path, serde_json::to_vec_pretty(&bundle).unwrap()).unwrap();

        let error = load_runtime_project(&output).unwrap_err();
        assert!(error.to_string().contains("cannot read bundled font"));
    }

    #[test]
    fn packaged_runtime_rejects_a_wrapper_that_lies_about_player_inclusion() {
        let temp = tempfile::tempdir().unwrap();
        project(temp.path());
        let output = temp.path().join("package");
        build_project(temp.path(), BuildTarget::LinuxX64, Some(&output)).unwrap();

        let manifest_path = output.join("build-manifest.json");
        let mut manifest: BuildManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.player_binary_included = false;
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = load_runtime_project(&output).unwrap_err();
        assert!(error.to_string().contains("Player inclusion"));
    }
}
