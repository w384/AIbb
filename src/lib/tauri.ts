import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
