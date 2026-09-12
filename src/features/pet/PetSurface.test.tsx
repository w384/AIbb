import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PetSurface } from "./PetSurface";
import {
  openArchiveWindow,
  openSettingsWindow,
  startPetDrag,
  toggleChatWindow,
  loadAibbProfile,
  listenProfileUpdated,
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
let profileListener: Listener<{ name: string; avatarDataUrl: string | null; version: number }>;
type DragDropEvent = { payload: { type: string; paths?: string[] } };
let dragDropHandler: ((event: DragDropEvent) => void) | undefined;
const petUnlisteners = [vi.fn(), vi.fn(), vi.fn(), vi.fn()];

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(async (handler: (event: DragDropEvent) => void) => {
      dragDropHandler = handler;
      return () => {};
    }),
  }),
}));

vi.mock("../../lib/tauri", () => ({
  openSettingsWindow: vi.fn(),
  startPetDrag: vi.fn(),
  toggleChatWindow: vi.fn(),
  openArchiveWindow: vi.fn(async () => undefined),
  loadAibbProfile: vi.fn(),
  listenProfileUpdated: vi.fn(async (listener: typeof profileListener) => {
    profileListener = listener;
    return petUnlisteners[0];
  }),
  listenExplorationProgress: vi.fn(async (listener: Listener<ExplorationProgressEvent>) => {
    progressListener = listener;
    return petUnlisteners[1];
  }),
  listenExplorationComplete: vi.fn(async (listener: Listener<ExplorationCompleteEvent>) => {
    completeListener = listener;
    return petUnlisteners[2];
  }),
  listenExplorationError: vi.fn(async (listener: Listener<ExplorationErrorEvent>) => {
    errorListener = listener;
    return petUnlisteners[3];
  }),
}));

const mockOpenSettings = vi.mocked(openSettingsWindow);
const mockStartPetDrag = vi.mocked(startPetDrag);
const mockToggleChat = vi.mocked(toggleChatWindow);
const mockOpenArchiveWindow = vi.mocked(openArchiveWindow);

describe("PetSurface", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: null,
      version: 0,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses left click for chat and right click for settings", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.click(pet, { detail: 1 });
    expect(mockToggleChat).toHaveBeenCalledTimes(1);

    expect(fireEvent.contextMenu(pet)).toBe(false);
    expect(mockOpenSettings).toHaveBeenCalledTimes(1);
    expect(mockToggleChat).toHaveBeenCalledTimes(1);
  });

  it("hands dropped files over to the archive window via the native drag-drop event", async () => {
    const view = render(<PetSurface status="idle" />);
    await waitFor(() => expect(dragDropHandler).toBeDefined());

    act(() => {
      dragDropHandler!({ payload: { type: "enter", paths: ["C:\\drop\\集成方案.pdf"] } });
    });
    expect(view.container.querySelector(".pet-surface")?.className).toContain(
      "drop-active",
    );

    act(() => {
      dragDropHandler!({ payload: { type: "drop", paths: ["C:\\drop\\集成方案.pdf"] } });
    });
    await waitFor(() =>
      expect(mockOpenArchiveWindow).toHaveBeenCalledWith([
        "C:\\drop\\集成方案.pdf",
      ]),
    );
  });

  it("shows a stable error when the chat window cannot open", async () => {
    mockToggleChat.mockRejectedValueOnce(new Error("native detail must not reach the pet"));
    const view = render(<PetSurface status="idle" />);

    fireEvent.click(within(view.container).getByRole("button", { name: "AIbb" }), {
      detail: 1,
    });

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

  it("updates the pet avatar and accessible name after profile update", async () => {
    const view = render(<PetSurface status="idle" />);

    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    act(() => profileListener({
      name: "小团子",
      avatarDataUrl: "data:image/webp;base64,AA==",
      version: 2,
    }));

    expect(within(view.container).getByRole("button", { name: "小团子" })).toBeVisible();
    expect(within(view.container).getByRole("img", { name: "小团子" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AA==",
    );
  });

  it("cleans up a resolved profile listener when another listener rejects", async () => {
    vi.mocked(listenExplorationProgress).mockRejectedValueOnce(new Error("registration failed"));
    const view = render(<PetSurface status="idle" />);

    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    view.unmount();

    await waitFor(() => expect(petUnlisteners[0]).toHaveBeenCalledTimes(1));
  });

  it("keeps the default identity when profile loading fails", async () => {
    vi.mocked(loadAibbProfile).mockRejectedValueOnce(new Error("profile unavailable"));
    const view = render(<PetSurface status="idle" />);

    await waitFor(() => expect(loadAibbProfile).toHaveBeenCalledTimes(1));
    expect(within(view.container).getByRole("button", { name: "AIbb" })).toBeVisible();
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
          diary: "回来啦",
          sources: [],
          roundNumber: 1,
          elapsedSeconds: 1,
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
          diary: "回来啦",
          sources: [],
          roundNumber: 1,
          elapsedSeconds: 1,
          rawResponse: "safe",
        },
      }),
    );
    expect(view.container.firstChild).toHaveAttribute("data-status", "returned");

    fireEvent.click(within(view.container).getByRole("button", { name: "AIbb" }), {
      detail: 1,
    });
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

  it("opens chat after a short primary press without starting a drag", () => {
    vi.useFakeTimers();
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
    act(() => vi.advanceTimersByTime(80));
    fireEvent.pointerUp(pet, { clientX: 15, clientY: 10, pointerId: 1 });
    fireEvent.click(pet, { detail: 1 });

    expect(mockStartPetDrag).not.toHaveBeenCalled();
    expect(mockToggleChat).toHaveBeenCalledTimes(1);
  });

  it("starts dragging only after a long primary press and suppresses its click", () => {
    vi.useFakeTimers();
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      isPrimary: true,
      pointerId: 7,
    });

    act(() => vi.advanceTimersByTime(99));
    expect(mockStartPetDrag).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(mockStartPetDrag).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(pet, { pointerId: 7 });
    fireEvent.click(pet, { detail: 1 });
    expect(mockToggleChat).not.toHaveBeenCalled();
  });

  it("starts dragging immediately when a held pointer begins to move", () => {
    vi.useFakeTimers();
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    fireEvent.pointerDown(pet, {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
      isPrimary: true,
      pointerId: 8,
    });
    fireEvent.pointerMove(pet, {
      buttons: 1,
      clientX: 13,
      clientY: 10,
      isPrimary: true,
      pointerId: 8,
    });

    expect(mockStartPetDrag).toHaveBeenCalledTimes(1);
    fireEvent.click(pet, { detail: 1 });
    expect(mockToggleChat).not.toHaveBeenCalled();
  });

  it("keeps right click dedicated to settings without starting a drag", () => {
    vi.useFakeTimers();
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
    fireEvent.contextMenu(pet);
    act(() => vi.advanceTimersByTime(500));

    expect(mockStartPetDrag).not.toHaveBeenCalled();
    expect(mockOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("cancels a pending long press when the pointer is cancelled", () => {
    vi.useFakeTimers();
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
    act(() => vi.advanceTimersByTime(500));

    expect(mockStartPetDrag).not.toHaveBeenCalled();
  });

  it("does not expose the removed drag handle or keyboard activation", () => {
    const view = render(<PetSurface status="idle" />);
    const pet = within(view.container).getByRole("button", { name: "AIbb" });

    expect(
      within(view.container).queryByRole("button", { name: "移动 AIbb" }),
    ).not.toBeInTheDocument();
    fireEvent.click(pet, { detail: 0 });

    expect(mockToggleChat).not.toHaveBeenCalled();
  });
});
