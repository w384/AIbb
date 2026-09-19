import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  ApiSettings,
  AibbProfile,
  ArchiveFileResult,
  ArchiveLedgerEntry,
  ArchiveSettings,
  BootstrapState,
  ChatCompleteEvent,
  ChatDeltaEvent,
  ChatErrorEvent,
  ChatHistory,
  DiscoveredStructure,
  ExplorationCompleteEvent,
  ExplorationDiaryDeltaEvent,
  ExplorationErrorEvent,
  ExplorationPageReadEvent,
  ExplorationProgressEvent,
  ExplorationQueryEvent,
  HistoryMessage,
  InputDisposition,
  OutingStats,
  SaveArchiveSettings,
  SaveSettings,
  VocabExportResult,
} from "../contracts";

type PayloadListener<T> = (payload: T) => void;

function listenFor<T>(eventName: string, listener: PayloadListener<T>): Promise<UnlistenFn> {
  return listen<T>(eventName, (event) => listener(event.payload));
}

export function toggleChatWindow(): Promise<void> {
  return invoke("toggle_chat_window");
}

/** Opens a link in the user's default browser (never inside the webview). */
export function openExternal(url: string): Promise<void> {
  return invoke("open_external_url", { url });
}

export function openSettingsWindow(): Promise<void> {
  return invoke("open_settings_window");
}

/** Starts a manual pet drag. The backend anchors the current window and
 * cursor positions and moves the window by set_position on every move so it
 * stays transparent while dragging — no white block, no rectangular snapshot.
 * The cursor is read by the backend, so the drag is exact on any DPI. */
export function petDragBegin(): Promise<void> {
  return invoke("pet_drag_begin");
}

export function petDragMove(): Promise<void> {
  return invoke("pet_drag_move");
}

/** Ends the drag: snaps to the nearest edge and remembers the position. */
export function petDragEnd(): Promise<void> {
  return invoke("pet_drag_end");
}

export function getBootstrapState(): Promise<BootstrapState> {
  return invoke("get_bootstrap_state");
}

export function getAppVersion(): Promise<string> {
  return getVersion();
}

export function submitUserInput(
  message: string,
  requestId: string,
): Promise<InputDisposition> {
  return invoke("submit_user_input", { message, requestId });
}

/** Sends a term to the vocabulary assistant (no outing-intent routing). */
export function submitVocabInput(
  message: string,
  requestId: string,
): Promise<void> {
  return invoke("submit_vocab_input", { message, requestId });
}

/** Recent vocabulary-assistant messages (vocab channel only). */
export function loadVocabHistory(limit?: number): Promise<HistoryMessage[]> {
  return invoke("load_vocab_history", { limit });
}

/** Tells the model to tidy the glossary into a table and writes it to disk. */
export function exportVocabGlossary(): Promise<VocabExportResult> {
  return invoke("export_vocab_glossary");
}

export function clearVocabMemory(): Promise<void> {
  return invoke("clear_vocab_memory");
}

export function loadSettings(): Promise<ApiSettings> {
  return invoke("load_settings");
}

export function loadAibbProfile(): Promise<AibbProfile> {
  return invoke("load_aibb_profile");
}

export function loadChatHistory(): Promise<ChatHistory> {
  return invoke("load_chat_history");
}

export function loadOutingStats(): Promise<OutingStats> {
  return invoke("outing_stats");
}

export function saveAibbName(name: string): Promise<AibbProfile> {
  return invoke("save_aibb_name", { name });
}

export function saveAibbAvatar(bytes: Uint8Array, mimeType: string): Promise<AibbProfile> {
  return invoke("save_aibb_avatar", { bytes: Array.from(bytes), mimeType });
}

export function resetAibbAvatar(): Promise<AibbProfile> {
  return invoke("reset_aibb_avatar");
}

export function saveSettings(settings: SaveSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export function testConnection(): Promise<void> {
  return invoke("test_connection");
}

export function listAvailableModels(): Promise<string[]> {
  return invoke("list_available_models");
}

export function clearMemory(): Promise<void> {
  return invoke("clear_memory");
}

export function exitApp(): Promise<void> {
  return invoke("exit_app");
}

export const listenChatDelta = (listener: PayloadListener<ChatDeltaEvent>) =>
  listenFor("chat://delta", listener);
export const listenChatComplete = (listener: PayloadListener<ChatCompleteEvent>) =>
  listenFor("chat://complete", listener);
export const listenChatError = (listener: PayloadListener<ChatErrorEvent>) =>
  listenFor("chat://error", listener);
export const listenExplorationProgress = (
  listener: PayloadListener<ExplorationProgressEvent>,
) => listenFor("exploration://progress", listener);
export const listenExplorationQuery = (
  listener: PayloadListener<ExplorationQueryEvent>,
) => listenFor("exploration://query", listener);
export const listenExplorationPageRead = (
  listener: PayloadListener<ExplorationPageReadEvent>,
) => listenFor("exploration://page-read", listener);
export const listenExplorationDiaryDelta = (
  listener: PayloadListener<ExplorationDiaryDeltaEvent>,
) => listenFor("exploration://diary-delta", listener);
export const listenExplorationComplete = (
  listener: PayloadListener<ExplorationCompleteEvent>,
) => listenFor("exploration://complete", listener);
export const listenExplorationError = (
  listener: PayloadListener<ExplorationErrorEvent>,
) => listenFor("exploration://error", listener);
export const listenProfileUpdated = (listener: PayloadListener<AibbProfile>) =>
  listenFor("profile://updated", listener);

/** Fired by the host when the pet click shows the chat window again, so the
 * window can land on the mode chooser (普通对话 / 词汇助手). */
export const listenChatOpened = (listener: () => void) =>
  listenFor<void>("chat://opened", listener);

/** Window focus changes (true = focused) — used to land at the latest
 * message whenever the chat window is activated again. */
export function listenChatWindowFocus(
  listener: (focused: boolean) => void,
): Promise<UnlistenFn> {
  return getCurrentWindow().onFocusChanged(({ payload: focused }) =>
    listener(focused),
  );
}

/** Fired by the host after a file drop stashes paths for the chat window. */
export const listenArchivePending = (listener: PayloadListener<number>) =>
  listenFor("archive://pending", listener);

export function openArchiveWindow(paths: string[]): Promise<void> {
  return invoke("open_archive_window", { paths });
}

export function takePendingArchivePaths(): Promise<string[]> {
  return invoke("take_pending_archive_paths");
}

export function archiveFiles(paths: string[], project: string): Promise<ArchiveFileResult[]> {
  return invoke("archive_files", { paths, project });
}

export function loadArchiveSettings(): Promise<ArchiveSettings> {
  return invoke("load_archive_settings");
}

export function saveArchiveSettings(settings: SaveArchiveSettings): Promise<void> {
  return invoke("save_archive_settings", { settings });
}

export function discoverArchiveStructure(): Promise<DiscoveredStructure | null> {
  return invoke("discover_archive_structure");
}

export function archiveLedger(limit: number): Promise<ArchiveLedgerEntry[]> {
  return invoke("archive_ledger", { limit });
}
