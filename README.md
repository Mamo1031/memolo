# memolo

会議を録音し、**文字起こし・要約まで完全ローカル**で行うデスクトップアプリ。
生成した議事録は任意で Notion に書き出せる（**オンライン処理はこの Notion 出力のみ**で、録音・文字起こし・要約はすべてローカル完結）。
8GB MacBook Air で動作することを目標に設計されている。

- **録音**: WebAudio + AudioWorklet で PCM 取得 → Rust で WAV 保存
- **文字起こし**: [faster-whisper](https://github.com/SYSTRAN/faster-whisper) (CTranslate2 int8)
- **要約**: ローカル LLM ([Qwen2.5-3B-Instruct](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF) GGUF, Metal バックエンド)
- **エクスポート (任意)**: 議事録を Notion ページに 1 クリックで出力

## 必要環境

- macOS 12+ (Apple Silicon 推奨)
- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) stable (1.88+)
- [uv](https://github.com/astral-sh/uv) (Python パッケージマネージャ)

```bash
# uv のインストール例
brew install uv
```

## セットアップ

```bash
# 1. フロントエンド依存
npm install

# 2. Python sidecar 環境 (faster-whisper + llama-cpp-python)
cd sidecar
uv python install 3.11
uv sync               # llama-cpp-python は Apple Silicon でソースビルドされる (数分かかる)
cd ..
```

初回起動時に以下のモデルが自動ダウンロードされます (キャッシュ先 `~/.cache/huggingface/`):

- faster-whisper medium (~764MB)
- Qwen2.5-3B-Instruct Q4_K_M (~1.9GB)

開発中に軽くしたい場合は環境変数で切替:

```bash
MEMOLO_WHISPER_MODEL=small npm run tauri dev   # whisper を small (~244MB) に
```

## 開発

```bash
npm run tauri dev
```

`tauri dev` がフロントエンド (Vite) と Rust バックエンドを並行起動し、Memolo ウィンドウが開きます。
初回はマイク権限プロンプトが出るので許可してください。

### 主要コマンド

| コマンド | 用途 |
|---|---|
| `npm run tauri dev` | 開発用起動 (ホットリロード) |
| `npm run tauri build` | リリースビルド (.app 生成。配布用の sidecar 同梱は未対応) |
| `npm run check` | Svelte/TS 型チェック |
| `cargo check` (in `src-tauri/`) | Rust コンパイル確認 |
| `cd sidecar && uv sync` | Python 依存の追加・更新 |

### sidecar 単体テスト

```bash
cd sidecar
# 文字起こし
printf '%s\n' '{"wav_path":"/abs/path.wav","language":"ja"}' | .venv/bin/python transcribe.py
# 要約
printf '%s\n' '{"transcript":"..."}' | .venv/bin/python summarize.py
```

## 出力先

録音 WAV: `~/Library/Application Support/app.memolo.desktop/recordings/<uuid>.wav`

## Notion 連携 (任意)

要約した議事録を Notion ページに書き出せる。**memolo がオンライン通信するのはこの機能だけ**で、録音・文字起こし・要約は完全ローカル。

1. [Notion のコネクション設定](https://www.notion.so/developers/connections)で新しいコネクション（旧称インテグレーション）を作成し、**Internal Integration Secret**（`ntn_…` / `secret_…`）をコピー
2. 書き出し先にしたい**親ページ**を Notion で開き、右上の **「•••」→「コネクトを追加」**で作成したコネクションを接続する（**未接続だと出力時に 404 になる**）
3. memolo の待機中／要約完了画面の **「⚙️ Notion 連携を設定」**を開き、トークンと親ページの URL を入力して保存（初回は macOS Keychain の許可ダイアログが出る）

出力するたびに、親ページの子ページとして `会議議事録 YYYY-MM-DD HH:MM` というページを新規作成する。議事録の `##` 見出し・箇条書きはそのまま Notion の見出し・リストに変換される。

- **トークン保存先**: macOS Keychain (`app.memolo.desktop` / `notion-token`)
- **親ページ ID 保存先**: `~/Library/Application Support/app.memolo.desktop/config.json`

## アーキテクチャ

```
src/lib/
├── recorder/    # MediaStream → AudioWorklet → Int16LE → base64 → invoke
├── transcribe/  # Channel<TranscribeEvent>
└── summarize/   # Channel<SummarizeEvent> + cancel

src-tauri/src/
├── recorder.rs    # hound で WAV 書き込み
├── transcribe.rs  # tokio::process で sidecar/transcribe.py を spawn
└── summarize.rs   # 同上 + Mutex<Option<Child>> でキャンセル対応

sidecar/
├── transcribe.py  # faster-whisper (stdin JSON / stdout JSON-Lines)
└── summarize.py   # llama-cpp-python + Qwen GGUF (同 IPC)
```

すべての sidecar IPC は **stdin: JSON 1行 / stdout: JSON-Lines (1行=1イベント)** で統一。
Rust 側は各イベントを Tauri の `Channel<T>` で Svelte へ転送する。
