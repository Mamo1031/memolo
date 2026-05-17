use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Local};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const KEYCHAIN_SERVICE: &str = "app.memolo.desktop";
const KEYCHAIN_USER: &str = "notion-token";
const NOTION_API_VERSION: &str = "2022-06-28";
const NOTION_BASE: &str = "https://api.notion.com/v1";

#[derive(Serialize, Deserialize, Default)]
struct ConfigJson {
    notion_parent_page_id: Option<String>,
}

#[derive(Serialize)]
pub struct NotionConfigStatus {
    configured: bool,
    parent_page_id: Option<String>,
}

#[derive(Serialize)]
pub struct NotionExportResult {
    url: String,
    page_id: String,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("create app data dir: {e}"))?;
    Ok(dir.join("config.json"))
}

fn read_config(app: &AppHandle) -> Result<ConfigJson, String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(ConfigJson::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read config: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse config: {e}"))
}

fn write_config(app: &AppHandle, cfg: &ConfigJson) -> Result<(), String> {
    let path = config_path(app)?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize config: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("write config: {e}"))
}

fn keychain_entry() -> Result<Entry, String> {
    Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|e| format!("keychain entry: {e}"))
}

/// URL や bare ID から 32-hex の Notion ID を抜き出し、8-4-4-4-12 形式に整形する。
fn normalize_page_id(raw: &str) -> Result<String, String> {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    // URL に複数の hex がある場合 (workspace id 等)、最後の 32 文字を採用する。
    if cleaned.len() < 32 {
        return Err("ページ ID または URL に 32 文字の hex が含まれていません".into());
    }
    let id = &cleaned[cleaned.len() - 32..];
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &id[0..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..32]
    ))
}

#[tauri::command]
pub fn notion_get_config(app: AppHandle) -> Result<NotionConfigStatus, String> {
    let cfg = read_config(&app)?;
    let token_exists = match keychain_entry()?.get_password() {
        Ok(_) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(e) => return Err(format!("keychain read: {e}")),
    };
    Ok(NotionConfigStatus {
        configured: token_exists && cfg.notion_parent_page_id.is_some(),
        parent_page_id: cfg.notion_parent_page_id,
    })
}

#[tauri::command]
pub fn notion_set_config(
    app: AppHandle,
    token: String,
    parent_page: String,
) -> Result<(), String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("トークンが空です".into());
    }
    let page_id = normalize_page_id(parent_page.trim())?;

    keychain_entry()?
        .set_password(&token)
        .map_err(|e| format!("keychain write: {e}"))?;

    let mut cfg = read_config(&app)?;
    cfg.notion_parent_page_id = Some(page_id);
    write_config(&app, &cfg)?;
    Ok(())
}

#[tauri::command]
pub fn notion_clear_config(app: AppHandle) -> Result<(), String> {
    match keychain_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(format!("keychain delete: {e}")),
    }
    let mut cfg = read_config(&app)?;
    cfg.notion_parent_page_id = None;
    write_config(&app, &cfg)?;
    Ok(())
}

fn build_title(started_at_iso: &str) -> String {
    let local = DateTime::parse_from_rfc3339(started_at_iso)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());
    format!("会議議事録 {}", local.format("%Y-%m-%d %H:%M"))
}

fn rich_text(content: &str) -> Vec<Value> {
    // Notion の rich_text content は 1 つあたり 2000 文字制限。
    if content.len() <= 2000 {
        return vec![json!({ "type": "text", "text": { "content": content } })];
    }
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = content.as_bytes();
    while start < bytes.len() {
        let mut end = (start + 2000).min(bytes.len());
        // UTF-8 マルチバイト境界で切る
        while end < bytes.len() && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
            end -= 1;
        }
        let slice = &content[start..end];
        out.push(json!({ "type": "text", "text": { "content": slice } }));
        start = end;
    }
    out
}

fn markdown_to_blocks(markdown: &str) -> Vec<Value> {
    let mut blocks = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            blocks.push(json!({
                "object": "block",
                "type": "heading_2",
                "heading_2": { "rich_text": rich_text(rest.trim()) }
            }));
        } else if let Some(rest) = trimmed.strip_prefix("### ") {
            blocks.push(json!({
                "object": "block",
                "type": "heading_3",
                "heading_3": { "rich_text": rich_text(rest.trim()) }
            }));
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            blocks.push(json!({
                "object": "block",
                "type": "heading_1",
                "heading_1": { "rich_text": rich_text(rest.trim()) }
            }));
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            blocks.push(json!({
                "object": "block",
                "type": "bulleted_list_item",
                "bulleted_list_item": { "rich_text": rich_text(rest.trim()) }
            }));
        } else {
            blocks.push(json!({
                "object": "block",
                "type": "paragraph",
                "paragraph": { "rich_text": rich_text(trimmed) }
            }));
        }
    }
    blocks
}

fn map_status_error(status: reqwest::StatusCode, body: &str) -> String {
    match status.as_u16() {
        401 => "Notion トークンが無効です。設定を見直してください。".into(),
        404 => "親ページが見つからないか、integration と共有されていません。Notion 側の Share でこの integration をページに追加してください。".into(),
        429 => "Notion API のレート制限に達しました。少し待って再試行してください。".into(),
        _ => format!("Notion API エラー (HTTP {status}): {body}"),
    }
}

#[tauri::command]
pub async fn notion_export(
    app: AppHandle,
    started_at_iso: String,
    markdown: String,
) -> Result<NotionExportResult, String> {
    let token = keychain_entry()?
        .get_password()
        .map_err(|e| match e {
            keyring::Error::NoEntry => "Notion トークンが未設定です".to_string(),
            other => format!("keychain read: {other}"),
        })?;

    let parent_id = read_config(&app)?
        .notion_parent_page_id
        .ok_or_else(|| "親ページ ID が未設定です".to_string())?;

    let title = build_title(&started_at_iso);
    let blocks = markdown_to_blocks(&markdown);

    let body = json!({
        "parent": { "page_id": parent_id },
        "properties": {
            "title": {
                "title": [{ "type": "text", "text": { "content": title } }]
            }
        },
        "children": blocks,
    });

    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("reqwest client: {e}"))?;

    let res = client
        .post(format!("{NOTION_BASE}/pages"))
        .bearer_auth(&token)
        .header("Notion-Version", NOTION_API_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Notion API request: {e}"))?;

    let status = res.status();
    let text = res
        .text()
        .await
        .map_err(|e| format!("read response body: {e}"))?;

    if !status.is_success() {
        return Err(map_status_error(status, &text));
    }

    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("parse Notion response: {e} body={text}"))?;

    let url = v
        .get("url")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("response missing url: {text}"))?
        .to_string();
    let page_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("response missing id: {text}"))?
        .to_string();

    Ok(NotionExportResult { url, page_id })
}
