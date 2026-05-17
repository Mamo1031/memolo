mod notion;
mod recorder;
mod summarize;
mod transcribe;

use notion::{notion_clear_config, notion_export, notion_get_config, notion_set_config};
use recorder::{audio_chunk, audio_start, audio_stop, RecorderState};
use summarize::{summarize, summarize_cancel, SummarizeState};
use transcribe::transcribe;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(RecorderState::default())
        .manage(SummarizeState::default())
        .invoke_handler(tauri::generate_handler![
            audio_start,
            audio_chunk,
            audio_stop,
            transcribe,
            summarize,
            summarize_cancel,
            notion_get_config,
            notion_set_config,
            notion_clear_config,
            notion_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
