import { invoke, Channel } from "@tauri-apps/api/core";

export type TranscribeEvent =
  | { type: "loaded"; model: string; device: string }
  | { type: "info"; duration: number; language: string; language_probability: number }
  | { type: "segment"; id: number; start: number; end: number; text: string }
  | { type: "done"; total_segments: number; full_text: string }
  | { type: "error"; message: string };

export async function transcribe(
  wavPath: string,
  onEvent: (e: TranscribeEvent) => void,
): Promise<void> {
  const onEventChannel = new Channel<TranscribeEvent>();
  onEventChannel.onmessage = onEvent;
  await invoke("transcribe", { wavPath, onEvent: onEventChannel });
}
