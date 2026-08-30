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

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 1,
    });
    fireEvent.pointerMove(pet, {
      buttons: 1,
      clientX: 15,
      clientY: 10,
      isPrimary: true,
      pointerId: 1,
    });
    fireEvent.pointerUp(pet, { clientX: 15, clientY: 10, pointerId: 1 });
    fireEvent.click(pet);

    expect(mockStartPetDrag).toHaveBeenCalledTimes(1);
    expect(mockToggleChat).not.toHaveBeenCalled();
  });

  it("does not drag from right-button movement", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 2,
      buttons: 2,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 2,
    });
    fireEvent.pointerMove(pet, {
      buttons: 2,
      clientX: 20,
      clientY: 10,
      isPrimary: true,
      pointerId: 2,
    });

    expect(mockStartPetDrag).not.toHaveBeenCalled();
  });

  it("does not drag after the pointer is cancelled", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 3,
    });
    fireEvent.pointerCancel(pet, { pointerId: 3 });
    fireEvent.pointerMove(pet, {
      buttons: 1,
      clientX: 20,
      clientY: 10,
      isPrimary: true,
      pointerId: 3,
    });

    expect(mockStartPetDrag).not.toHaveBeenCalled();
  });

  it("does not drag after pointer capture is lost", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 4,
    });
    fireEvent.lostPointerCapture(pet, { pointerId: 4 });
    fireEvent.pointerMove(pet, {
      buttons: 1,
      clientX: 20,
      clientY: 10,
      isPrimary: true,
      pointerId: 4,
    });

    expect(mockStartPetDrag).not.toHaveBeenCalled();
  });

  it("does not drag when the primary button is no longer pressed", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 5,
    });
    fireEvent.pointerMove(pet, {
      buttons: 0,
      clientX: 20,
      clientY: 10,
      isPrimary: true,
      pointerId: 5,
    });

    expect(mockStartPetDrag).not.toHaveBeenCalled();
  });
});
