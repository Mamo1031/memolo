use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct SummarizeState {
    pub child: Mutex<Option<Child>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SummarizeEvent {
    Downloading {
        repo: String,
        file: String,
    },
    Loaded {
        model: String,
        backend: String,
    },
    Token {
        text: String,
    },
    Done {
        summary: String,
    },
    Error {
        message: String,
    },
}

fn resolve_sidecar() -> Result<(PathBuf, PathBuf), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    let project_root = cwd
        .parent()
        .ok_or_else(|| "cwd has no parent".to_string())?
        .to_path_buf();
    let python = project_root.join("sidecar/.venv/bin/python");
    let script = project_root.join("sidecar/summarize.py");
    Ok((python, script))
}

#[tauri::command]
pub async fn summarize(
    state: State<'_, SummarizeState>,
    transcript: String,
    on_event: Channel<SummarizeEvent>,
) -> Result<(), String> {
    let (python, script) = resolve_sidecar()?;

    if !python.exists() {
        return Err(format!(
            "sidecar venv が見つかりません: {}\n\
             `cd sidecar && uv sync` を実行してください。",
            python.display()
        ));
    }
    if !script.exists() {
        return Err(format!("summarize.py が見つかりません: {}", script.display()));
    }

    let mut child = Command::new(&python)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("spawn summarize sidecar: {e}"))?;

    let cfg = serde_json::json!({ "transcript": transcript });
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
    } // drop stdin → EOF

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout".to_string())?;

    // child を state にセット (cancel から触れるように)
    *state.child.lock().await = Some(child);

    let mut lines = BufReader::new(stdout).lines();
    let mut read_err: Option<String> = None;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<SummarizeEvent>(&line) {
                    Ok(ev) => {
                        if let Err(e) = on_event.send(ev) {
                            eprintln!("channel send: {e}");
                        }
                    }
                    Err(e) => eprintln!("summarize JSON parse error: {e} line={line}"),
                }
            }
            Ok(None) => break,
            Err(e) => {
                read_err = Some(format!("read stdout: {e}"));
                break;
            }
        }
    }

    // child を state から取り出して wait
    let waited = {
        let mut guard = state.child.lock().await;
        if let Some(mut c) = guard.take() {
            Some(c.wait().await)
        } else {
            None
        }
    };

    // child が state から消えていれば、それは summarize_cancel が奪ったということ。
    // この場合 stdout 読みは EOF/Err どちらも cancelled として扱う。
    let cancelled = waited.is_none();
    if cancelled {
        return Err("cancelled".into());
    }

    if let Some(err) = read_err {
        return Err(err);
    }

    match waited {
        Some(Ok(status)) if status.success() => Ok(()),
        Some(Ok(status)) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if status.signal().is_some() {
                    return Err("cancelled".into());
                }
            }
            Err(format!("summarize sidecar exited with {status}"))
        }
        Some(Err(e)) => Err(format!("wait sidecar: {e}")),
        None => unreachable!("checked above"),
    }
}

#[tauri::command]
pub async fn summarize_cancel(state: State<'_, SummarizeState>) -> Result<(), String> {
    let mut guard = state.child.lock().await;
    if let Some(mut child) = guard.take() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    Ok(())
}
