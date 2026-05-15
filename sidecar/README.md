# memolo sidecar

memolo の音声文字起こしを行う Python プロセス。
Tauri 側から `tokio::process::Command` で spawn され、stdin で設定を受け取り、stdout に JSON-Lines を吐く。

## セットアップ

[uv](https://github.com/astral-sh/uv) を使う前提。

```bash
# 初回のみ
uv python install 3.11
uv venv --python 3.11
uv pip install -e .
```

## 単体テスト

```bash
printf '%s\n' '{"wav_path":"/abs/path/to/audio.wav","language":"ja"}' \
  | .venv/bin/python transcribe.py
```

期待出力 (1行=1 JSON イベント):

```json
{"type": "loaded", "model": "medium", "device": "cpu"}
{"type": "info", "duration": 18.4, "language": "ja", "language_probability": 0.99}
{"type": "segment", "id": 0, "start": 0.0, "end": 2.4, "text": "..."}
...
{"type": "done", "total_segments": 12, "full_text": "..."}
```

## 環境変数

- `MEMOLO_WHISPER_MODEL`: 使うモデル。`tiny` / `base` / `small` / `medium` / `large-v3` 等。default は stdin の `model_size`、最終 default は `medium`。dev 中の高速化に `small` 推奨。
