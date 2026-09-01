import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  ApiSettings,
  BootstrapState,
  ChatCompleteEvent,
  ChatDeltaEvent,
  ChatErrorEvent,
  ExplorationCompleteEvent,
  ExplorationErrorEvent,
  ExplorationProgressEvent,
  InputDisposition,
  SaveSettings,
} from "../contracts";

type PayloadListener<T> = (payload: T) => void;

function listenFor<T>(eventName: string, listener: PayloadListener<T>): Promise<UnlistenFn> {
  return listen<T>(eventName, (event) => listener(event.payload));
}

export function toggleChatWindow(): Promise<void> {
  return invoke("toggle_chat_window");
}

export function openSettingsWindow(): Promise<void> {
  return invoke("open_settings_window");
}

export async function startPetDrag(): Promise<void> {
  await invoke("start_pet_drag");
  const position = await getCurrentWindow().outerPosition();
  await savePetPosition(position.x, position.y);
}

export function savePetPosition(x: number, y: number): Promise<void> {
  return invoke("save_pet_position", { x, y });
}

export function getBootstrapState(): Promise<BootstrapState> {
  return invoke("get_bootstrap_state");
}

export function submitUserInput(
  message: string,
  requestId: string,
): Promise<InputDisposition> {
  return invoke("submit_user_input", { message, requestId });
}

export function startExploration(direction?: string): Promise<string> {
  return invoke("start_exploration", {
    request: { direction: direction ?? null },
  });
}

export function cancelExploration(taskId: string): Promise<void> {
  return invoke("cancel_exploration", { taskId });
}

export function loadSettings(): Promise<ApiSettings> {
  return invoke("load_settings");
}

export function saveSettings(settings: SaveSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export function testConnection(): Promise<void> {
  return invoke("test_connection");
}

export function clearMemory(): Promise<void> {
  return invoke("clear_memory");
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
export const listenExplorationComplete = (
  listener: PayloadListener<ExplorationCompleteEvent>,
) => listenFor("exploration://complete", listener);
export const listenExplorationError = (
  listener: PayloadListener<ExplorationErrorEvent>,
) => listenFor("exploration://error", listener);
