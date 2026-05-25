<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { Recorder, type RecorderEvent } from "$lib/recorder/recorder";
  import { transcribe, type TranscribeEvent } from "$lib/transcribe/transcribe";
  import {
    summarize,
    summarizeCancel,
    type SummarizeEvent,
  } from "$lib/summarize/summarize";
  import {
    getConfig as getNotionConfig,
    setConfig as setNotionConfig,
    clearConfig as clearNotionConfig,
    exportToNotion,
  } from "$lib/notion/notion";

  type Phase =
    | "idle"
    | "recording"
    | "saving"
    | "transcribing"
    | "transcribed"
    | "summarizing"
    | "summarized"
    | "error";

  type Segment = { id: number; start: number; end: number; text: string };

  let phase = $state<Phase>("idle");
  let elapsedMs = $state(0);
  let level = $state(0);
  let savedPath = $state("");
  let errorMessage = $state("");

  // transcribe state
  let modelInfo = $state("");
  let audioInfo = $state("");
  let segments = $state<Segment[]>([]);
  let fullText = $state("");

  // summarize state
  let summarizeStatus = $state("");
  let streamingSummary = $state("");
  let finalSummary = $state("");

  // notion state
  let notionConfigured = $state(false);
  let notionParentId = $state<string | null>(null);
  let notionModalOpen = $state(false);
  let notionTokenInput = $state("");
  let notionParentInput = $state("");
  let notionSaving = $state(false);
  let notionModalError = $state("");
  let notionExporting = $state(false);
  let notionResultUrl = $state("");

  let recorder: Recorder | null = null;
  let timerId: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;
  let segmentsEl = $state<HTMLElement | null>(null);
  let summaryEl = $state<HTMLTextAreaElement | null>(null);

  const phaseLabel: Record<Phase, string> = {
    idle: "待機中",
    recording: "録音中",
    saving: "保存中…",
    transcribing: "文字起こし中…",
    transcribed: "文字起こし完了",
    summarizing: "要約中…",
    summarized: "要約完了",
    error: "エラー",
  };

  function fmtElapsed(ms: number): string {
    const total = Math.floor(ms / 1000);
    const m = String(Math.floor(total / 60)).padStart(2, "0");
    const s = String(total % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  function fmtSec(sec: number): string {
    const total = Math.floor(sec);
    const m = String(Math.floor(total / 60)).padStart(2, "0");
    const s = String(total % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  async function refreshNotionStatus() {
    try {
      const s = await getNotionConfig();
      notionConfigured = s.configured;
      notionParentId = s.parent_page_id;
    } catch (e) {
      // keychain アクセス拒否などのケース。UI は未設定扱いにする。
      notionConfigured = false;
      notionParentId = null;
    }
  }

  onMount(() => {
    refreshNotionStatus();
  });

  function onRecorderEvent(e: RecorderEvent) {
    if (e.type === "level") level = e.rms;
    else if (e.type === "error") {
      errorMessage = e.message;
      phase = "error";
    }
  }

  async function startRecording() {
    errorMessage = "";
    savedPath = "";
    level = 0;
    modelInfo = "";
    audioInfo = "";
    segments = [];
    fullText = "";
    summarizeStatus = "";
    streamingSummary = "";
    finalSummary = "";
    notionResultUrl = "";

    recorder = new Recorder(onRecorderEvent);
    try {
      await recorder.start();
      phase = "recording";
      startedAt = Date.now();
      elapsedMs = 0;
      timerId = setInterval(() => {
        elapsedMs = Date.now() - startedAt;
      }, 200);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("NotAllowed") || msg.includes("Permission")) {
        errorMessage =
          "マイクへのアクセスを許可してください。システム設定 → プライバシーとセキュリティ → マイク。";
      } else {
        errorMessage = msg;
      }
      phase = "error";
      recorder = null;
    }
  }

  async function stopRecording() {
    if (!recorder) return;
    phase = "saving";
    if (timerId) {
      clearInterval(timerId);
      timerId = null;
    }
    try {
      const path = await recorder.stop();
      savedPath = path;
      recorder = null;
      await runTranscribe(path);
    } catch (e) {
      errorMessage = e instanceof Error ? e.message : String(e);
      phase = "error";
      recorder = null;
    }
  }

  async function runTranscribe(wavPath: string) {
    phase = "transcribing";
    modelInfo = "モデル読み込み中…";
    audioInfo = "";
    segments = [];
    fullText = "";

    try {
      await transcribe(wavPath, (e: TranscribeEvent) => {
        switch (e.type) {
          case "loaded":
            modelInfo = `モデル ${e.model} (${e.device})`;
            break;
          case "info":
            audioInfo = `${e.duration.toFixed(1)}s · ${e.language} (${Math.round(e.language_probability * 100)}%)`;
            break;
          case "segment":
            segments = [
              ...segments,
              { id: e.id, start: e.start, end: e.end, text: e.text },
            ];
            queueMicrotask(() => {
              segmentsEl?.scrollTo({ top: segmentsEl.scrollHeight });
            });
            break;
          case "done":
            fullText = e.full_text;
            phase = "transcribed";
            break;
          case "error":
            errorMessage = e.message;
            phase = "error";
            break;
        }
      });
    } catch (e) {
      errorMessage = e instanceof Error ? e.message : String(e);
      phase = "error";
    }
  }

  async function runSummarize() {
    if (!fullText.trim()) return;
    phase = "summarizing";
    summarizeStatus = "準備中…";
    streamingSummary = "";
    finalSummary = "";
    errorMessage = "";
    notionResultUrl = "";

    try {
      await summarize(fullText, (e: SummarizeEvent) => {
        switch (e.type) {
          case "downloading":
            summarizeStatus = `モデル準備中 (${e.file})…`;
            break;
          case "loaded":
            summarizeStatus = `要約生成中 (${e.model}, ${e.backend})`;
            break;
          case "token":
            streamingSummary += e.text;
            queueMicrotask(() => {
              if (summaryEl) summaryEl.scrollTop = summaryEl.scrollHeight;
            });
            break;
          case "done":
            finalSummary = e.summary;
            phase = "summarized";
            break;
          case "error":
            errorMessage = e.message;
            phase = "error";
            break;
        }
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg === "cancelled") {
        phase = "transcribed";
        streamingSummary = "";
        summarizeStatus = "";
      } else {
        errorMessage = msg;
        phase = "error";
      }
    }
  }

  async function cancelSummarize() {
    try {
      await summarizeCancel();
    } catch {
      // ignore
    }
  }

  function openNotionModal() {
    notionTokenInput = "";
    notionParentInput = notionParentId ?? "";
    notionModalError = "";
    notionModalOpen = true;
  }

  function closeNotionModal() {
    notionModalOpen = false;
    notionTokenInput = "";
    notionModalError = "";
  }

  async function saveNotionSettings() {
    if (notionSaving) return;
    notionSaving = true;
    notionModalError = "";
    try {
      await setNotionConfig(notionTokenInput, notionParentInput);
      await refreshNotionStatus();
      notionModalOpen = false;
      notionTokenInput = "";
    } catch (e) {
      notionModalError = e instanceof Error ? e.message : String(e);
    } finally {
      notionSaving = false;
    }
  }

  async function clearNotionSettings() {
    try {
      await clearNotionConfig();
      await refreshNotionStatus();
      notionResultUrl = "";
    } catch (e) {
      errorMessage = e instanceof Error ? e.message : String(e);
    }
  }

  async function doNotionExport() {
    if (notionExporting || !finalSummary) return;
    notionExporting = true;
    notionResultUrl = "";
    errorMessage = "";
    try {
      const startedIso = new Date(startedAt || Date.now()).toISOString();
      const r = await exportToNotion(startedIso, finalSummary);
      notionResultUrl = r.url;
    } catch (e) {
      errorMessage = e instanceof Error ? e.message : String(e);
    } finally {
      notionExporting = false;
    }
  }

  async function openExternal(url: string) {
    try {
      await openUrl(url);
    } catch {
      // ignore
    }
  }

  function reset() {
    phase = "idle";
    elapsedMs = 0;
    level = 0;
    savedPath = "";
    errorMessage = "";
    modelInfo = "";
    audioInfo = "";
    segments = [];
    fullText = "";
    summarizeStatus = "";
    streamingSummary = "";
    finalSummary = "";
    notionResultUrl = "";
  }

  async function copyFullText() {
    if (!fullText) return;
    try {
      await navigator.clipboard.writeText(fullText);
    } catch {
      // ignore
    }
  }

  async function copySummary() {
    const text = finalSummary || streamingSummary;
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // ignore
    }
  }

  onDestroy(() => {
    if (timerId) clearInterval(timerId);
    if (recorder) recorder.stop().catch(() => {});
    summarizeCancel().catch(() => {});
  });

  const meterPct = $derived(Math.min(100, Math.round(level * 300)));
  const canRecord = $derived(
    phase === "idle" ||
      phase === "transcribed" ||
      phase === "summarized" ||
      phase === "error",
  );
  const showTranscribe = $derived(
    phase === "transcribing" ||
      phase === "transcribed" ||
      phase === "summarizing" ||
      phase === "summarized",
  );
</script>

<main>
  <h1>Memolo</h1>
  <p class="status">{phaseLabel[phase]}</p>

  {#if phase === "recording"}
    <p class="elapsed">{fmtElapsed(elapsedMs)}</p>
    <div class="meter">
      <div class="meter-fill" style="width: {meterPct}%"></div>
    </div>
  {:else if phase === "transcribed" || phase === "summarized"}
    <p class="elapsed muted">{fmtElapsed(elapsedMs)}</p>
  {/if}

  <div class="actions">
    {#if canRecord}
      <button class="record" onclick={startRecording}>● 録音開始</button>
    {:else if phase === "recording"}
      <button class="stop" onclick={stopRecording}>■ 停止</button>
    {:else if phase === "summarizing"}
      <button class="cancel" onclick={cancelSummarize}>キャンセル</button>
    {:else}
      <button class="record" disabled>{phaseLabel[phase]}</button>
    {/if}

    {#if phase === "transcribed" || phase === "summarized" || phase === "error"}
      <button class="ghost" onclick={reset}>リセット</button>
    {/if}
  </div>

  {#if showTranscribe}
    <section class="block">
      <header class="block-head">
        <span class="muted small">文字起こし</span>
        {#if modelInfo || audioInfo}
          <span class="meta">
            {#if modelInfo}<span>{modelInfo}</span>{/if}
            {#if audioInfo}<span class="dot">·</span><span>{audioInfo}</span>{/if}
          </span>
        {/if}
      </header>

      {#if segments.length > 0}
        <div class="segments" bind:this={segmentsEl}>
          {#each segments as seg (seg.id)}
            <div class="segment">
              <span class="t">[{fmtSec(seg.start)}]</span>
              <span class="seg-text">{seg.text}</span>
            </div>
          {/each}
        </div>
      {:else if phase === "transcribing"}
        <p class="muted small center">最初の segment を待っています…</p>
      {/if}

      {#if (phase === "transcribed" || phase === "summarizing" || phase === "summarized") && fullText}
        <div class="ft-head">
          <span class="muted small">全文</span>
          <div class="btn-row">
            <button class="link" onclick={copyFullText}>コピー</button>
            {#if phase === "transcribed"}
              <button class="primary" onclick={runSummarize}>✨ 要約する</button>
            {/if}
          </div>
        </div>
        <textarea class="full" readonly value={fullText}></textarea>
      {/if}
    </section>
  {/if}

  {#if phase === "summarizing" || phase === "summarized"}
    <section class="block">
      <header class="block-head">
        <span class="muted small">議事録</span>
        {#if summarizeStatus}<span class="meta"><span>{summarizeStatus}</span></span>{/if}
      </header>

      {#if phase === "summarizing"}
        <textarea
          class="summary"
          readonly
          bind:this={summaryEl}
          value={streamingSummary || "(モデル読み込み中…)"}
        ></textarea>
      {:else}
        <textarea class="summary" readonly value={finalSummary}></textarea>
        <div class="btn-row right">
          <button class="link" onclick={copySummary}>コピー</button>
          <button class="ghost-sm" onclick={runSummarize}>再要約</button>
          {#if notionConfigured}
            <button
              class="primary"
              onclick={doNotionExport}
              disabled={notionExporting}
            >
              {notionExporting ? "出力中…" : "📤 Notion に出力"}
            </button>
          {:else}
            <button class="ghost-sm" onclick={openNotionModal}>⚙️ Notion 連携を設定</button>
          {/if}
        </div>

        {#if notionResultUrl}
          <div class="notion-result">
            <span>✅ Notion に出力しました</span>
            <button class="link" onclick={() => openExternal(notionResultUrl)}>
              ページを開く
            </button>
          </div>
        {/if}

        {#if notionConfigured}
          <div class="notion-foot">
            <span class="muted small">Notion 連携: 設定済み</span>
            <button class="link tiny" onclick={openNotionModal}>変更</button>
            <button class="link tiny" onclick={clearNotionSettings}>クリア</button>
          </div>
        {/if}
      {/if}
    </section>
  {/if}

  {#if showTranscribe && savedPath}
    <section class="result">
      <p class="muted small">保存先</p>
      <code>{savedPath}</code>
    </section>
  {/if}

  {#if errorMessage}
    <section class="error">
      <p>{errorMessage}</p>
    </section>
  {/if}
</main>

{#if notionModalOpen}
  <div
    class="modal-backdrop"
    role="dialog"
    aria-modal="true"
    onclick={closeNotionModal}
    onkeydown={(e) => e.key === "Escape" && closeNotionModal()}
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="modal"
      role="document"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h2>Notion 連携を設定</h2>
      <p class="muted small">
        Notion でコネクション（旧インテグレーション）を作成し、書き出し先の親ページの「•••」→「コネクトを追加」でそのコネクションを接続してください。
        トークンは macOS Keychain に安全に保存されます。
      </p>
      <button
        class="link tiny"
        onclick={() => openExternal("https://www.notion.so/developers/connections")}
      >
        コネクション作成ページを開く →
      </button>

      <label>
        <span>コネクションのトークン</span>
        <input
          type="password"
          placeholder="ntn_xxxxxxxx… または secret_xxxxxxxx…"
          bind:value={notionTokenInput}
          autocomplete="off"
        />
      </label>

      <label>
        <span>親ページの URL または ID</span>
        <input
          type="text"
          placeholder="https://www.notion.so/Your-Page-abcdef1234..."
          bind:value={notionParentInput}
          autocomplete="off"
        />
      </label>

      {#if notionModalError}
        <p class="modal-error">{notionModalError}</p>
      {/if}

      <div class="modal-actions">
        <button class="ghost" onclick={closeNotionModal} disabled={notionSaving}>
          キャンセル
        </button>
        <button
          class="primary"
          onclick={saveNotionSettings}
          disabled={notionSaving || !notionTokenInput.trim() || !notionParentInput.trim()}
        >
          {notionSaving ? "保存中…" : "保存"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  :root {
    color-scheme: light dark;
  }

  main {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 1.5rem 1.5rem 2rem;
    font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif;
    gap: 0.9rem;
    background: #fafafa;
    color: #1a1a1a;
  }

  h1 {
    font-size: 1.3rem;
    margin: 0;
    letter-spacing: 0.04em;
    font-weight: 600;
  }

  .status {
    margin: 0;
    color: #555;
    font-size: 0.9rem;
  }

  .elapsed {
    font-variant-numeric: tabular-nums;
    font-size: 2.4rem;
    margin: 0;
    font-weight: 300;
  }

  .elapsed.muted {
    color: #666;
    font-size: 1.2rem;
  }

  .meter {
    width: 200px;
    height: 6px;
    background: #e6e6e6;
    border-radius: 3px;
    overflow: hidden;
  }

  .meter-fill {
    height: 100%;
    background: linear-gradient(to right, #4caf50, #ff9800, #f44336);
    transition: width 60ms linear;
  }

  .actions {
    display: flex;
    gap: 0.6rem;
    align-items: center;
  }

  button {
    border: none;
    border-radius: 999px;
    font-size: 0.95rem;
    font-weight: 500;
    padding: 0.75rem 1.4rem;
    cursor: pointer;
    transition: transform 0.05s ease, background 0.2s ease;
  }

  button:active:not(:disabled) {
    transform: scale(0.97);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .record {
    background: #e53935;
    color: white;
  }

  .record:hover:not(:disabled) {
    background: #c62828;
  }

  .stop {
    background: #1a1a1a;
    color: white;
  }

  .stop:hover {
    background: #333;
  }

  .cancel {
    background: #ff9800;
    color: white;
  }

  .cancel:hover {
    background: #f57c00;
  }

  .ghost {
    background: transparent;
    color: #555;
    border: 1px solid #ccc;
  }

  .ghost:hover {
    background: #f0f0f0;
  }

  .ghost-sm,
  .link {
    padding: 0.3rem 0.7rem;
    font-size: 0.78rem;
    background: transparent;
    border: 1px solid #ccc;
    border-radius: 6px;
    color: #555;
  }

  .ghost-sm:hover,
  .link:hover {
    background: #f0f0f0;
  }

  .link.tiny {
    padding: 0.18rem 0.45rem;
    font-size: 0.72rem;
  }

  .primary {
    background: #2962ff;
    color: white;
    padding: 0.35rem 0.85rem;
    font-size: 0.85rem;
    border-radius: 999px;
  }

  .primary:hover:not(:disabled) {
    background: #1d4fd1;
  }

  .block {
    width: 100%;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .block-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .meta {
    display: flex;
    gap: 0.35rem;
    font-size: 0.78rem;
    color: #777;
    flex-wrap: wrap;
    text-align: right;
  }

  .meta .dot {
    color: #bbb;
  }

  .segments {
    max-height: 160px;
    overflow-y: auto;
    border: 1px solid #e0e0e0;
    border-radius: 8px;
    background: #fff;
    padding: 0.5rem 0.7rem;
    font-size: 0.86rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .segment {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    line-height: 1.4;
  }

  .segment .t {
    font-variant-numeric: tabular-nums;
    color: #999;
    flex-shrink: 0;
    font-size: 0.75rem;
  }

  .seg-text {
    word-break: break-word;
  }

  .ft-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 0.2rem;
  }

  .btn-row {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }

  .btn-row.right {
    justify-content: flex-end;
    flex-wrap: wrap;
  }

  .full,
  .summary {
    width: 100%;
    resize: vertical;
    padding: 0.6rem 0.7rem;
    border: 1px solid #e0e0e0;
    border-radius: 8px;
    background: #fff;
    color: #1a1a1a;
    font-size: 0.85rem;
    line-height: 1.5;
    font-family: inherit;
  }

  .full {
    min-height: 80px;
  }

  .summary {
    min-height: 180px;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    font-size: 0.82rem;
  }

  .notion-result {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.65rem;
    background: #e8f5e9;
    border: 1px solid #c8e6c9;
    border-radius: 8px;
    font-size: 0.85rem;
    color: #1b5e20;
  }

  .notion-foot {
    display: flex;
    gap: 0.4rem;
    align-items: center;
    justify-content: flex-end;
    margin-top: 0.25rem;
  }

  .result {
    text-align: center;
    max-width: 90%;
  }

  .result code {
    display: inline-block;
    padding: 0.35rem 0.55rem;
    background: #fff;
    border: 1px solid #e0e0e0;
    border-radius: 6px;
    font-size: 0.7rem;
    word-break: break-all;
    color: #555;
  }

  .error {
    max-width: 90%;
    padding: 0.7rem 0.95rem;
    background: #ffebee;
    border: 1px solid #ffcdd2;
    border-radius: 8px;
    color: #b71c1c;
    font-size: 0.88rem;
  }

  .error p {
    margin: 0;
  }

  .muted {
    color: #777;
  }

  .small {
    font-size: 0.75rem;
    margin: 0;
  }

  .center {
    text-align: center;
  }

  /* Notion modal */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 1rem;
  }

  .modal {
    background: #fff;
    color: #1a1a1a;
    border-radius: 12px;
    padding: 1.25rem;
    width: 100%;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
  }

  .modal h2 {
    margin: 0;
    font-size: 1.05rem;
  }

  .modal label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.82rem;
    color: #444;
  }

  .modal input {
    border: 1px solid #d0d0d0;
    border-radius: 6px;
    padding: 0.5rem 0.65rem;
    font-size: 0.88rem;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    background: #fff;
    color: #1a1a1a;
  }

  .modal input:focus {
    outline: 2px solid #2962ff;
    outline-offset: -1px;
  }

  .modal-error {
    margin: 0;
    padding: 0.5rem 0.7rem;
    background: #ffebee;
    border: 1px solid #ffcdd2;
    border-radius: 6px;
    color: #b71c1c;
    font-size: 0.82rem;
  }

  .modal-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    margin-top: 0.4rem;
  }

  @media (prefers-color-scheme: dark) {
    main {
      background: #1a1a1a;
      color: #f0f0f0;
    }
    .status {
      color: #aaa;
    }
    .elapsed.muted {
      color: #999;
    }
    .meter {
      background: #333;
    }
    .ghost,
    .ghost-sm,
    .link {
      color: #ccc;
      border-color: #444;
    }
    .ghost:hover,
    .ghost-sm:hover,
    .link:hover {
      background: #2a2a2a;
    }
    .segments,
    .full,
    .summary {
      background: #222;
      border-color: #333;
      color: #ddd;
    }
    .result code {
      background: #222;
      border-color: #333;
      color: #aaa;
    }
    .meta {
      color: #aaa;
    }
    .meta .dot {
      color: #555;
    }
    .segment .t {
      color: #777;
    }
    .error {
      background: #3b1f1f;
      border-color: #5b2a2a;
      color: #ff8a80;
    }
    .muted {
      color: #aaa;
    }
    .notion-result {
      background: #1b3a1f;
      border-color: #2c5530;
      color: #a5d6a7;
    }
    .modal {
      background: #2a2a2a;
      color: #f0f0f0;
    }
    .modal label {
      color: #ccc;
    }
    .modal input {
      background: #1f1f1f;
      border-color: #444;
      color: #f0f0f0;
    }
  }
</style>
