use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hound::{SampleFormat, WavSpec, WavWriter};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

type WavBufWriter = WavWriter<BufWriter<File>>;

#[derive(Default)]
pub struct RecorderState {
    sessions: Mutex<HashMap<String, WavBufWriter>>,
    paths: Mutex<HashMap<String, PathBuf>>,
}

#[derive(Serialize)]
pub struct StartResult {
    session_id: String,
    path: String,
}

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let dir = base.join("recordings");
    create_dir_all(&dir).map_err(|e| format!("create recordings dir: {e}"))?;
    Ok(dir)
}

#[tauri::command]
pub fn audio_start(
    state: State<'_, RecorderState>,
    app: AppHandle,
    sample_rate: u32,
) -> Result<StartResult, String> {
    let dir = recordings_dir(&app)?;
    let id = Uuid::new_v4().to_string();
    let path = dir.join(format!("{id}.wav"));

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let writer = WavWriter::create(&path, spec)
        .map_err(|e| format!("create wav: {e}"))?;

    state
        .sessions
        .lock()
        .map_err(|e| format!("lock sessions: {e}"))?
        .insert(id.clone(), writer);
    state
        .paths
        .lock()
        .map_err(|e| format!("lock paths: {e}"))?
        .insert(id.clone(), path.clone());

    Ok(StartResult {
        session_id: id,
        path: path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn audio_chunk(
    state: State<'_, RecorderState>,
    session_id: String,
    data_b64: String,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;
    if bytes.len() % 2 != 0 {
        return Err(format!("odd byte length: {}", bytes.len()));
    }

    let mut sessions = state
        .sessions
        .lock()
        .map_err(|e| format!("lock sessions: {e}"))?;
    let writer = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("unknown session: {session_id}"))?;

    for chunk in bytes.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        writer
            .write_sample(sample)
            .map_err(|e| format!("write sample: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn audio_stop(
    state: State<'_, RecorderState>,
    session_id: String,
) -> Result<String, String> {
    let writer = state
        .sessions
        .lock()
        .map_err(|e| format!("lock sessions: {e}"))?
        .remove(&session_id)
        .ok_or_else(|| format!("unknown session: {session_id}"))?;
    writer.finalize().map_err(|e| format!("finalize wav: {e}"))?;

    let path = state
        .paths
        .lock()
        .map_err(|e| format!("lock paths: {e}"))?
        .remove(&session_id)
        .ok_or_else(|| format!("unknown session path: {session_id}"))?;
    Ok(path.to_string_lossy().to_string())
}
