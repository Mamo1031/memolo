"""memolo 音声文字起こし sidecar.

stdin: 改行終端の JSON 1行
  {"wav_path": "/abs/path.wav", "language": "ja", "model_size": "medium"}
  - model_size は env MEMOLO_WHISPER_MODEL があればそれで上書き

stdout: JSON-Lines (1行=1イベント, ensure_ascii=False, flush=True)
  {"type": "loaded",  "model": "medium", "device": "cpu"}
  {"type": "info",    "duration": 18.4, "language": "ja", "language_probability": 0.99}
  {"type": "segment", "id": 0, "start": 0.0, "end": 2.4, "text": "..."}
  ...
  {"type": "done",    "total_segments": 12, "full_text": "..."}
  {"type": "error",   "message": "..."}   # 例外時 (exit 1)

stderr: faster-whisper 自身のログを含むデバッグ出力
"""

from __future__ import annotations

import json
import os
import sys
import traceback


def emit(event: dict) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def main() -> int:
    raw = sys.stdin.readline()
    if not raw.strip():
        emit({"type": "error", "message": "no config on stdin"})
        return 1

    try:
        cfg = json.loads(raw)
    except json.JSONDecodeError as e:
        emit({"type": "error", "message": f"stdin JSON parse: {e}"})
        return 1

    wav_path = cfg.get("wav_path")
    if not wav_path:
        emit({"type": "error", "message": "wav_path is required"})
        return 1

    model_size = os.environ.get("MEMOLO_WHISPER_MODEL") or cfg.get("model_size", "medium")
    language = cfg.get("language", "ja")

    try:
        from faster_whisper import WhisperModel
    except ImportError as e:
        emit({"type": "error", "message": f"faster-whisper not installed: {e}"})
        return 1

    try:
        model = WhisperModel(model_size, device="cpu", compute_type="int8")
        emit({"type": "loaded", "model": model_size, "device": "cpu"})

        segments_iter, info = model.transcribe(
            wav_path,
            language=language,
            vad_filter=True,
        )
        emit({
            "type": "info",
            "duration": float(info.duration),
            "language": info.language,
            "language_probability": float(info.language_probability),
        })

        texts: list[str] = []
        for i, seg in enumerate(segments_iter):
            texts.append(seg.text)
            emit({
                "type": "segment",
                "id": i,
                "start": float(seg.start),
                "end": float(seg.end),
                "text": seg.text,
            })

        emit({"type": "done", "total_segments": len(texts), "full_text": "".join(texts)})
        return 0
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        emit({"type": "error", "message": f"{type(e).__name__}: {e}"})
        return 1


if __name__ == "__main__":
    sys.exit(main())
