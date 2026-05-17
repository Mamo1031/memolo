// AudioWorkletProcessor: mono入力をFloat32バッファに溜め、4096サンプル単位でmain threadへ転送する。
// 注意: AudioWorklet global scope では import/export 不可。素のJSとして配信される必要があるため static/ に置く。

const FRAME_SIZE = 4096;

class RecorderProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buffer = new Float32Array(FRAME_SIZE);
    this.offset = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const channel = input[0];
    if (!channel) return true;

    for (let i = 0; i < channel.length; i++) {
      this.buffer[this.offset++] = channel[i];
      if (this.offset >= FRAME_SIZE) {
        this.port.postMessage(this.buffer.slice(0));
        this.offset = 0;
      }
    }
    return true;
  }
}

registerProcessor("recorder-processor", RecorderProcessor);
