#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

const KEEP_GENERATIONS: usize = 3;

#[derive(Default)]
struct SaveState(Mutex<()>);

#[derive(Debug, Clone, Serialize)]
struct SaveGeneration {
    generation: u64,
    payload: String,
}

#[tauri::command]
fn save_generation(
    app: AppHandle,
    state: State<'_, SaveState>,
    namespace: String,
    slot: String,
    payload: String,
) -> Result<u64, String> {
    let _guard = state.0.lock().map_err(|_| "save store lock was poisoned")?;
    let directory = save_slot_directory(&app, &namespace, &slot)?;
    fs::create_dir_all(&directory).map_err(io_error)?;
    let mut generation_paths = generation_paths(&directory)?;
    let next = generation_paths
        .first()
        .map_or(1, |(generation, _)| generation.saturating_add(1));
    let path = directory.join(format!("{next:020}.json"));
    atomic_write(&path, payload.as_bytes())?;
    generation_paths.insert(0, (next, path));
    for (_, stale_path) in generation_paths.into_iter().skip(KEEP_GENERATIONS) {
        if stale_path.exists() {
            fs::remove_file(stale_path).map_err(io_error)?;
        }
    }
    Ok(next)
}

#[tauri::command]
fn load_generations(
    app: AppHandle,
    state: State<'_, SaveState>,
    namespace: String,
    slot: String,
) -> Result<Vec<SaveGeneration>, String> {
    let _guard = state.0.lock().map_err(|_| "save store lock was poisoned")?;
    let directory = save_slot_directory(&app, &namespace, &slot)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    read_generations(&directory)
}

#[tauri::command]
fn load_latest_generation(
    app: AppHandle,
    state: State<'_, SaveState>,
    namespace: String,
    slot: String,
) -> Result<Option<SaveGeneration>, String> {
    let _guard = state.0.lock().map_err(|_| "save store lock was poisoned")?;
    let directory = save_slot_directory(&app, &namespace, &slot)?;
    if !directory.exists() {
        return Ok(None);
    }
    let Some((generation, path)) = generation_paths(&directory)?.into_iter().next() else {
        return Ok(None);
    };
    let payload = fs::read_to_string(path).map_err(io_error)?;
    Ok(Some(SaveGeneration {
        generation,
        payload,
    }))
}

#[tauri::command]
fn purge_save_namespace(
    app: AppHandle,
    state: State<'_, SaveState>,
    namespace: String,
) -> Result<bool, String> {
    let _guard = state.0.lock().map_err(|_| "save store lock was poisoned")?;
    let directory = save_namespace_directory(&app, &namespace)?;
    if !directory.exists() {
        return Ok(true);
    }
    if !directory.is_dir() {
        return Err("save namespace path is not a directory".to_owned());
    }
    fs::remove_dir_all(directory).map_err(io_error)?;
    Ok(true)
}

fn save_slot_directory(app: &AppHandle, namespace: &str, slot: &str) -> Result<PathBuf, String> {
    Ok(save_namespace_directory(app, namespace)?.join(save_slot_component(slot)?))
}

fn save_slot_component(slot: &str) -> Result<String, String> {
    if slot == "autosave" {
        return Ok("autosave".to_owned());
    }
    let manual = slot
        .parse::<u32>()
        .map_err(|_| "invalid save slot".to_owned())?;
    if !(1..=10).contains(&manual) {
        return Err("invalid save slot".to_owned());
    }
    Ok(format!("slot-{manual}"))
}

fn save_namespace_directory(app: &AppHandle, namespace: &str) -> Result<PathBuf, String> {
    if namespace.is_empty()
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("invalid save namespace".to_owned());
    }
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    Ok(root.join("saves").join(namespace))
}

fn read_generations(directory: &Path) -> Result<Vec<SaveGeneration>, String> {
    let paths = generation_paths(directory)?;
    let mut generations = Vec::with_capacity(paths.len());
    for (generation, path) in paths {
        let payload = fs::read_to_string(path).map_err(io_error)?;
        generations.push(SaveGeneration {
            generation,
            payload,
        });
    }
    Ok(generations)
}

fn generation_paths(directory: &Path) -> Result<Vec<(u64, PathBuf)>, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if !entry.file_type().map_err(io_error)?.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(generation) = stem.parse::<u64>() else {
            continue;
        };
        paths.push((generation, path));
    }
    paths.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(paths)
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "save path has no parent".to_owned())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = parent.join(format!(".save-{nonce}.tmp"));
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io_error)?;
    file.write_all(contents).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    fs::rename(&temporary, path).map_err(io_error)
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::save_slot_component;

    #[test]
    fn automatic_checkpoint_has_a_dedicated_directory() {
        assert_eq!(save_slot_component("autosave").unwrap(), "autosave");
        assert_eq!(save_slot_component("1").unwrap(), "slot-1");
        assert!(save_slot_component("0").is_err());
        assert!(save_slot_component("11").is_err());
    }
}

fn main() {
    tauri::Builder::default()
        .manage(SaveState::default())
        .invoke_handler(tauri::generate_handler![
            save_generation,
            load_generations,
            load_latest_generation,
            purge_save_namespace
        ])
        .run(tauri::generate_context!())
        .expect("error while running Umikaze desktop application");
}
