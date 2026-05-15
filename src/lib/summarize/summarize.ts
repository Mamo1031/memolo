import { invoke, Channel } from "@tauri-apps/api/core";

export type SummarizeEvent =
  | { type: "downloading"; repo: string; file: string }
  | { type: "loaded"; model: string; backend: string }
  | { type: "token"; text: string }
  | { type: "done"; summary: string }
  | { type: "error"; message: string };

export async function summarize(
  transcript: string,
  onEvent: (e: SummarizeEvent) => void,
): Promise<void> {
  const ch = new Channel<SummarizeEvent>();
  ch.onmessage = onEvent;
  await invoke("summarize", { transcript, onEvent: ch });
}

export async function summarizeCancel(): Promise<void> {
  await invoke("summarize_cancel");
}
