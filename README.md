# memolo

会議を録音し、**完全ローカル**で文字起こし・要約するデスクトップアプリ。
8GB MacBook Air で動作することを目標に設計されている。

- **録音**: WebAudio + AudioWorklet で PCM 取得 → Rust で WAV 保存
- **文字起こし**: [faster-whisper](https://github.com/SYSTRAN/faster-whisper) (CTranslate2 int8)
- **要約**: ローカル LLM ([Qwen2.5-3B-Instruct](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF) GGUF, Metal バックエンド)

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
| `npm run tauri build` | リリースビルド (.app / .dmg 生成、要 Slice 4+) |
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
