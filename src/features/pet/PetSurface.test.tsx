import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PetSurface } from "./PetSurface";
import {
  openSettingsWindow,
  startPetDrag,
  toggleChatWindow,
  listenExplorationComplete,
  listenExplorationError,
  listenExplorationProgress,
} from "../../lib/tauri";
import type {
  ExplorationCompleteEvent,
  ExplorationErrorEvent,
  ExplorationProgressEvent,
} from "../../contracts";

type Listener<T> = (payload: T) => void;
let progressListener: Listener<ExplorationProgressEvent>;
let completeListener: Listener<ExplorationCompleteEvent>;
let errorListener: Listener<ExplorationErrorEvent>;
const petUnlisteners = [vi.fn(), vi.fn(), vi.fn()];

vi.mock("../../lib/tauri", () => ({
  openSettingsWindow: vi.fn(),
  startPetDrag: vi.fn(),
  toggleChatWindow: vi.fn(),
  listenExplorationProgress: vi.fn(async (listener: Listener<ExplorationProgressEvent>) => {
    progressListener = listener;
    return petUnlisteners[0];
  }),
  listenExplorationComplete: vi.fn(async (listener: Listener<ExplorationCompleteEvent>) => {
    completeListener = listener;
    return petUnlisteners[1];
  }),
  listenExplorationError: vi.fn(async (listener: Listener<ExplorationErrorEvent>) => {
    errorListener = listener;
    return petUnlisteners[2];
  }),
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

  it("opens chat on a left-button release before the browser dispatches click", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 90,
      clientY: 100,
      isPrimary: true,
      pointerId: 9,
    });
    fireEvent.pointerUp(pet, {
      button: 0,
      buttons: 0,
      clientX: 90,
      clientY: 100,
      isPrimary: true,
      pointerId: 9,
    });
    expect(mockToggleChat).toHaveBeenCalledTimes(1);
  });

  it("shows a stable error when the chat window cannot open", async () => {
    mockToggleChat.mockRejectedValueOnce(new Error("native detail must not reach the pet"));
    const view = render(<PetSurface status="idle" />);

    fireEvent.click(within(view.container).getByRole("button", { name: "AIbb" }));

    expect(await within(view.container).findByRole("alert")).toHaveTextContent(
      "暂时没能打开对话，请再点一次试试。",
    );
  });

  it("shows a short return bubble for a completed exploration", () => {
    const view = render(<PetSurface status="returned" />);

    expect(within(view.container).getByRole("status")).toHaveTextContent(
      "我回来啦，点我看结果",
    );
  });

  it("tracks real exploration events and resets returned only after chat opens", async () => {
    const view = render(<PetSurface status="idle" />);
    await waitFor(() => expect(listenExplorationProgress).toHaveBeenCalled());

    act(() => progressListener({ taskId: "task-1", status: "reading" }));
    expect(view.container.firstChild).toHaveAttribute("data-status", "exploring");
    act(() =>
      errorListener({ taskId: "other", code: "cancelled", message: "safe" }),
    );
    expect(view.container.firstChild).toHaveAttribute("data-status", "exploring");
    act(() =>
      completeListener({
        taskId: "other",
        result: {
          items: ["甲", "乙", "丙", "丁"],
          nextOutingRequest: "再去玩",
          rawResponse: "safe",
        },
      }),
    );
    expect(view.container.firstChild).toHaveAttribute("data-status", "exploring");
    act(() =>
      completeListener({
        taskId: "task-1",
        result: {
          items: ["甲", "乙", "丙", "丁"],
          nextOutingRequest: "再去玩",
          rawResponse: "safe",
        },
      }),
    );
    expect(view.container.firstChild).toHaveAttribute("data-status", "returned");

    fireEvent.click(within(view.container).getByRole("button", { name: "AIbb" }));
    await waitFor(() =>
      expect(view.container.firstChild).toHaveAttribute("data-status", "idle"),
    );
    expect(mockToggleChat).toHaveBeenCalledTimes(1);

    act(() => errorListener({ taskId: "task-1", code: "failed", message: "safe" }));
    expect(view.container.firstChild).toHaveAttribute("data-status", "idle");
    view.unmount();
    await waitFor(() =>
      petUnlisteners.forEach((unlisten) => expect(unlisten).toHaveBeenCalled()),
    );
    expect(listenExplorationComplete).toHaveBeenCalled();
    expect(listenExplorationError).toHaveBeenCalled();
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
