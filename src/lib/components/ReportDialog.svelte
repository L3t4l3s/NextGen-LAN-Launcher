<script lang="ts">
  import type { ConfigReport } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { api, copyText } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import Icon from "./Icon.svelte";

  // A problem report about one game or (without `gameId`) the launcher:
  // the same ways out as sharing a configuration — mail, GitHub issue,
  // copy, file.
  let {
    gameId = null,
    title,
    confirmedProfile = false,
    onclose,
  }: { gameId?: string | null; title: string; confirmedProfile?: boolean; onclose: () => void } = $props();

  // Edited after building, the report is built again: what goes out is
  // what was written.
  let comment = $state("");
  let report = $state<ConfigReport | null>(null);
  let working = $state(false);
  let copied = $state(false);
  const email = $derived(report ? report.mailto.slice("mailto:".length).split("?")[0] : "");

  async function build() {
    working = true;
    try {
      report = await api.bugReport(gameId, comment);
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      working = false;
    }
  }

  async function open(url: string) {
    try {
      await api.openUrl(url);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function copy() {
    if (!report) return;
    try {
      await copyText(`${report.subject}\n\n${report.body}`);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function saveFile() {
    if (!report) return;
    try {
      const path = await api.exportGameConfig(report.fileName, report.body);
      if (path) app.toast("success", t("config.share.file_saved", { path }));
    } catch (e) {
      app.toast("error", userText(e));
    }
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={onclose} onkeydown={(e) => e.key === "Escape" && onclose()}>
  <div class="modal card report" role="dialog" tabindex="-1" aria-labelledby="report-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
    <h2 id="report-title"><Icon name="bug" size={20} /> {t("report.title", { title })}</h2>
    {#if confirmedProfile}
      <p class="hint">{t("report.confirmed_profile")}</p>
    {/if}
    <label for="report-comment">{t("report.comment")}</label>
    <textarea id="report-comment" rows="4" bind:value={comment} oninput={() => (report = null)} placeholder={t(gameId ? "report.comment_placeholder_game" : "report.comment_placeholder")}></textarea>
    <p class="hint">{t("report.privacy")}</p>
    <div class="row end">
      <button onclick={build} disabled={working || !comment.trim()}>{t(report ? "config.share.rebuild" : "config.share.build")}</button>
    </div>
    {#if report}
      <pre class="preview">{report.body}</pre>
      <div class="row">
        <button class="primary" onclick={() => open(report!.mailto)}>{t("config.share.mail")}</button>
        <button onclick={() => open(report!.issueUrl)}>{t("config.share.issue")}</button>
        <button onclick={copy}>{copied ? t("action.copied") : t("config.share.copy")}</button>
        <button onclick={saveFile}>{t("config.share.file")}</button>
      </div>
      <p class="hint">{t("report.fallback", { email })}</p>
    {/if}
    <div class="row end">
      <button data-gamepad-back onclick={onclose}>{t("action.close")}</button>
    </div>
  </div>
</div>

<style>
  .report {
    width: min(640px, 94vw);
    max-height: 90vh;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0;
  }
  textarea {
    width: 100%;
    resize: vertical;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .row.end {
    justify-content: flex-end;
  }
  .preview {
    max-height: 14rem;
    overflow: auto;
    white-space: pre-wrap;
    font-size: 0.78rem;
    margin: 0;
    padding: 0.6rem;
    background: color-mix(in srgb, currentColor 6%, transparent);
    border-radius: 6px;
  }
</style>
