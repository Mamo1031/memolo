import { invoke } from "@tauri-apps/api/core";
import { bytesToBase64, floatToInt16Bytes } from "./pcm-encoder";

type StartResult = { session_id: string; path: string };

// AudioWorkletから上がってくるFloat32フレームをこの秒数ぶん溜めてから1回のinvokeで送る。
const FLUSH_SECONDS = 1.0;

export type RecorderEvent =
  | { type: "level"; rms: number }
  | { type: "error"; message: string };

export class Recorder {
  private ctx: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private node: AudioWorkletNode | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private sessionId: string | null = null;
  private pending: Float32Array[] = [];
  private pendingSamples = 0;
  private flushTarget = 0;
  private flushing: Promise<void> = Promise.resolve();
  private onEvent?: (e: RecorderEvent) => void;

  constructor(onEvent?: (e: RecorderEvent) => void) {
    this.onEvent = onEvent;
  }

  async start(): Promise<{ sessionId: string; sampleRate: number; path: string }> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
      video: false,
    });

    this.ctx = new AudioContext();
    const sampleRate = this.ctx.sampleRate;
    this.flushTarget = Math.max(1024, Math.floor(sampleRate * FLUSH_SECONDS));

    await this.ctx.audioWorklet.addModule("/recorder-worklet.js");
    this.node = new AudioWorkletNode(this.ctx, "recorder-processor", {
      numberOfInputs: 1,
      numberOfOutputs: 0,
      channelCount: 1,
    });
    this.node.port.onmessage = (ev) => this.onFrame(ev.data as Float32Array);

    const started = await invoke<StartResult>("audio_start", { sampleRate });
    this.sessionId = started.session_id;

    this.source = this.ctx.createMediaStreamSource(this.stream);
    this.source.connect(this.node);

    return { sessionId: started.session_id, sampleRate, path: started.path };
  }

  private onFrame(frame: Float32Array) {
    this.pending.push(frame);
    this.pendingSamples += frame.length;

    let sumSq = 0;
    for (let i = 0; i < frame.length; i++) sumSq += frame[i] * frame[i];
    this.onEvent?.({ type: "level", rms: Math.sqrt(sumSq / frame.length) });

    if (this.pendingSamples >= this.flushTarget) {
      this.scheduleFlush();
    }
  }

  private scheduleFlush() {
    const merged = this.drainPending();
    if (!merged || !this.sessionId) return;
    const sessionId = this.sessionId;
    this.flushing = this.flushing.then(async () => {
      try {
        const bytes = floatToInt16Bytes(merged);
        const dataB64 = bytesToBase64(bytes);
        await invoke("audio_chunk", { sessionId, dataB64 });
      } catch (e) {
        this.onEvent?.({ type: "error", message: String(e) });
      }
    });
  }

  private drainPending(): Float32Array | null {
    if (this.pending.length === 0) return null;
    const total = this.pendingSamples;
    const out = new Float32Array(total);
    let off = 0;
    for (const f of this.pending) {
      out.set(f, off);
      off += f.length;
    }
    this.pending = [];
    this.pendingSamples = 0;
    return out;
  }

  async stop(): Promise<string> {
    if (!this.sessionId) throw new Error("not started");
    const sessionId = this.sessionId;

    this.source?.disconnect();
    this.node?.disconnect();
    this.stream?.getTracks().forEach((t) => t.stop());

    // 残バッファを1回flushしてからstopを呼ぶ。
    this.scheduleFlush();
    await this.flushing;

    const path = await invoke<string>("audio_stop", { sessionId });

    await this.ctx?.close();
    this.ctx = null;
    this.stream = null;
    this.node = null;
    this.source = null;
    this.sessionId = null;

    return path;
  }
}
