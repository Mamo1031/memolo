// Float32 PCM (-1.0..1.0) を 16-bit LE PCM (Int16) に変換し、base64 文字列で返す。
// Tauri 2 の invoke は JSON ベースなので Vec<u8> 直渡しより base64 のほうが安定 (33%オーバーヘッドのみ)。

export function floatToInt16Bytes(samples: Float32Array): Uint8Array {
  const out = new Uint8Array(samples.length * 2);
  const view = new DataView(out.buffer);
  for (let i = 0; i < samples.length; i++) {
    let s = samples[i];
    if (s > 1) s = 1;
    else if (s < -1) s = -1;
    const i16 = s < 0 ? Math.round(s * 0x8000) : Math.round(s * 0x7fff);
    view.setInt16(i * 2, i16, true);
  }
  return out;
}

export function bytesToBase64(bytes: Uint8Array): string {
  // 大きなバッファでは String.fromCharCode(...bytes) がスタック制限に当たるためチャンク化。
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}
