import { fireEvent, render, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PetSurface } from "./PetSurface";
import {
  openSettingsWindow,
  startPetDrag,
  toggleChatWindow,
} from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({
  openSettingsWindow: vi.fn(),
  startPetDrag: vi.fn(),
  toggleChatWindow: vi.fn(),
}));

const mockOpenSettings = vi.mocked(openSettingsWindow);
const mockStartPetDrag = vi.mocked(startPetDrag);
const mockToggleChat = vi.mocked(toggleChatWindow);

describe("PetSurface", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses left click for chat and right click for settings", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.click(pet);
    expect(mockToggleChat).toHaveBeenCalledTimes(1);

    expect(fireEvent.contextMenu(pet)).toBe(false);
    expect(mockOpenSettings).toHaveBeenCalledTimes(1);
    expect(mockToggleChat).toHaveBeenCalledTimes(1);
  });

  it("starts dragging after movement exceeds four pixels and suppresses chat", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(pet, { clientX: 15, clientY: 10, pointerId: 1 });
    fireEvent.pointerUp(pet, { clientX: 15, clientY: 10, pointerId: 1 });
    fireEvent.click(pet);

    expect(mockStartPetDrag).toHaveBeenCalledTimes(1);
    expect(mockToggleChat).not.toHaveBeenCalled();
  });
});
