import { invoke } from "@tauri-apps/api/core";

export type NotionConfigStatus = {
  configured: boolean;
  parent_page_id: string | null;
};

export type NotionExportResult = {
  url: string;
  page_id: string;
};

export const getConfig = (): Promise<NotionConfigStatus> =>
  invoke<NotionConfigStatus>("notion_get_config");

export const setConfig = (token: string, parentPage: string): Promise<void> =>
  invoke<void>("notion_set_config", { token, parentPage });

export const clearConfig = (): Promise<void> => invoke<void>("notion_clear_config");

export const exportToNotion = (
  startedAtIso: string,
  markdown: string,
): Promise<NotionExportResult> =>
  invoke<NotionExportResult>("notion_export", { startedAtIso, markdown });
