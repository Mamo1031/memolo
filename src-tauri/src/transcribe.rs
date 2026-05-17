use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscribeEvent {
    Loaded {
        model: String,
        device: String,
    },
    Info {
        duration: f64,
        language: String,
        language_probability: f64,
    },
    Segment {
        id: u32,
        start: f64,
        end: f64,
        text: String,
    },
    Done {
        total_segments: u32,
        full_text: String,
    },
    Error {
        message: String,
    },
}

fn resolve_sidecar() -> Result<(PathBuf, PathBuf), String> {
    // dev 時の current_dir は src-tauri/。その親 (= memolo/) 配下の sidecar/ を見る。
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    let project_root = cwd
        .parent()
        .ok_or_else(|| "cwd has no parent".to_string())?
        .to_path_buf();
    let python = project_root.join("sidecar/.venv/bin/python");
    let script = project_root.join("sidecar/transcribe.py");
    Ok((python, script))
}

#[tauri::command]
pub async fn transcribe(
    wav_path: String,
    on_event: Channel<TranscribeEvent>,
) -> Result<(), String> {
    let (python, script) = resolve_sidecar()?;

    if !python.exists() {
        return Err(format!(
            "sidecar venv が見つかりません: {}\n\
             プロジェクト直下で `cd sidecar && uv venv --python 3.11 && uv sync` を実行してください。",
            python.display()
        ));
    }
    if !script.exists() {
        return Err(format!("sidecar スクリプトが見つかりません: {}", script.display()));
    }

    let mut child = Command::new(&python)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("spawn sidecar: {e}"))?;

    let cfg = serde_json::json!({
        "wav_path": wav_path,
        "language": "ja",
    });
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "no stdin".to_string())?;
        stdin
            .write_all(format!("{}\n", cfg).as_bytes())
            .await
            .map_err(|e| format!("write stdin: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("flush stdin: {e}"))?;
    } // drop stdin → EOF を sidecar に通知

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("read stdout: {e}"))?
    {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TranscribeEvent>(&line) {
            Ok(ev) => {
                if let Err(e) = on_event.send(ev) {
                    eprintln!("channel send: {e}");
                }
            }
            Err(e) => {
                eprintln!("sidecar JSON parse error: {e} line={line}");
            }
        }
    }

    let status = child.wait().await.map_err(|e| format!("wait sidecar: {e}"))?;
    if !status.success() {
        return Err(format!("sidecar exited with {status}"));
    }
    Ok(())
}
