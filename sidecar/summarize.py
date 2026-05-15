"""memolo 会議要約 sidecar (ローカル LLM, llama-cpp-python).

stdin: 改行終端の JSON 1行
  {"transcript": "...", "model_repo": "...", "model_file": "..."}
  - model_repo / model_file は環境変数 MEMOLO_LLM_MODEL_REPO / MEMOLO_LLM_MODEL_FILE で上書き可

stdout: JSON-Lines (1行=1イベント, ensure_ascii=False, flush=True)
  {"type": "downloading", "repo": "...", "file": "..."}    # DL 開始 (キャッシュヒット時もこのイベントは出る)
  {"type": "loaded",      "model": "...",  "backend": "metal"}
  {"type": "token",       "text": "..."}                    # chunk 単位
  {"type": "done",        "summary": "<完成 markdown>"}
  {"type": "error",       "message": "..."}                  # 例外時 (exit 1)
"""

from __future__ import annotations

import json
import os
import sys
import traceback


DEFAULT_REPO = "Qwen/Qwen2.5-3B-Instruct-GGUF"
DEFAULT_FILE = "qwen2.5-3b-instruct-q4_k_m.gguf"

SYSTEM_PROMPT = (
    "あなたは会議の議事録を作成するアシスタントです。"
    "文字起こしに書かれている事実だけを根拠に、簡潔な議事録を作成します。"
    "推測・補完・創作は一切行わないでください。"
)

USER_TEMPLATE = """以下の会議文字起こしを、ちょうど次の3つの見出しに分けた markdown 議事録にまとめてください。

出力フォーマット (この見出し以外は出力しない):

## トピック
- 議論された主なテーマを箇条書きで1行ずつ

## 決定事項
- 合意・決定された事項を箇条書きで。無ければ「- なし」とだけ書く

## アクションアイテム
- 誰が・何を・いつまでにやるかを箇条書きで。無ければ「- なし」とだけ書く

ルール:
- 上記の3つの見出しだけを出力する
- コードブロック(```)で囲まない
- 文字起こしに無い情報を補わない
- 各項目は1行で完結させる

# 文字起こし
{transcript}
"""


def emit(event: dict) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def strip_code_fence(text: str) -> str:
    """Qwen は出力を ```markdown ... ``` で囲むことがある。done の summary だけクリーンに返す。"""
    s = text.strip()
    if s.startswith("```"):
        first_nl = s.find("\n")
        if first_nl != -1:
            s = s[first_nl + 1 :]
    if s.endswith("```"):
        s = s[: -3].rstrip()
    return s


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

    transcript = cfg.get("transcript", "").strip()
    if not transcript:
        emit({"type": "error", "message": "transcript is required (empty)"})
        return 1

    repo = os.environ.get("MEMOLO_LLM_MODEL_REPO") or cfg.get("model_repo") or DEFAULT_REPO
    fname = os.environ.get("MEMOLO_LLM_MODEL_FILE") or cfg.get("model_file") or DEFAULT_FILE

    try:
        from huggingface_hub import hf_hub_download
    except ImportError as e:
        emit({"type": "error", "message": f"huggingface_hub not installed: {e}"})
        return 1

    try:
        emit({"type": "downloading", "repo": repo, "file": fname})
        model_path = hf_hub_download(repo_id=repo, filename=fname)

        from llama_cpp import Llama  # 重いので emit 後に import

        llm = Llama(
            model_path=model_path,
            n_ctx=8192,
            n_threads=os.cpu_count() or 4,
            n_gpu_layers=-1,
            verbose=False,
        )
        emit({"type": "loaded", "model": fname, "backend": "metal"})

        messages = [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": USER_TEMPLATE.format(transcript=transcript)},
        ]

        pieces: list[str] = []
        stream = llm.create_chat_completion(
            messages=messages,
            stream=True,
            temperature=0.2,
            top_p=0.9,
            max_tokens=2048,
            stop=["\n## アクションアイテム\n\n##", "```\n"],
        )
        for chunk in stream:
            delta = chunk["choices"][0]["delta"].get("content", "")
            if delta:
                pieces.append(delta)
                emit({"type": "token", "text": delta})

        emit({"type": "done", "summary": strip_code_fence("".join(pieces))})
        return 0
    except KeyboardInterrupt:
        # Rust 側からの SIGTERM もここに来ない場合があるが念のため
        emit({"type": "error", "message": "interrupted"})
        return 130
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        emit({"type": "error", "message": f"{type(e).__name__}: {e}"})
        return 1


if __name__ == "__main__":
    sys.exit(main())
