import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatCompleteEvent,
  ChatDeltaEvent,
  ChatErrorEvent,
} from "../../contracts";
import { ChatPanel } from "./ChatPanel";
import {
  getBootstrapState,
  listenChatComplete,
  listenChatDelta,
  listenChatError,
  openSettingsWindow,
  submitUserInput,
} from "../../lib/tauri";

type Listener<T> = (payload: T) => void;
let deltaListener: Listener<ChatDeltaEvent>;
let completeListener: Listener<ChatCompleteEvent>;
let errorListener: Listener<ChatErrorEvent>;
const unlistenDelta = vi.fn();
const unlistenComplete = vi.fn();
const unlistenError = vi.fn();

vi.mock("../../lib/tauri", () => ({
  getBootstrapState: vi.fn(),
  openSettingsWindow: vi.fn(),
  submitUserInput: vi.fn(),
  listenChatDelta: vi.fn(async (listener: Listener<ChatDeltaEvent>) => {
    deltaListener = listener;
    return unlistenDelta;
  }),
  listenChatComplete: vi.fn(async (listener: Listener<ChatCompleteEvent>) => {
    completeListener = listener;
    return unlistenComplete;
  }),
  listenChatError: vi.fn(async (listener: Listener<ChatErrorEvent>) => {
    errorListener = listener;
    return unlistenError;
  }),
}));

const mockBootstrap = vi.mocked(getBootstrapState);
const mockSubmit = vi.mocked(submitUserInput);

describe("ChatPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockBootstrap.mockResolvedValue({
      firstRun: false,
      apiConfigured: true,
      petStatus: "idle",
    });
  });

  it("shows the local greeting before API configuration without submitting to a model", async () => {
    mockBootstrap.mockResolvedValue({
      firstRun: true,
      apiConfigured: false,
      petStatus: "idle",
    });

    render(<ChatPanel />);

    expect(await screen.findByText(/喜欢出去玩耍的快乐 AIbb/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "打开 API 设置" }));
    expect(openSettingsWindow).toHaveBeenCalledTimes(1);
    expect(mockSubmit).not.toHaveBeenCalled();
  });

  it("presents a branded chat header with a direct settings action", async () => {
    render(<ChatPanel />);

    expect(await screen.findByRole("heading", { name: "AIbb" })).toBeVisible();
    expect(screen.getByText("准备出去玩")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));

    expect(openSettingsWindow).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("textbox", { name: "消息" })).toBeVisible();
  });

  it("streams only the active request and unregisters every listener on unmount", async () => {
    mockSubmit.mockImplementation(async (_message, id) => ({
      kind: "chatStarted",
      requestId: id,
    }));
    const view = render(<ChatPanel />);
    await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenChatDelta).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByRole("textbox", { name: "消息" }), {
      target: { value: "你好" },
    });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() =>
      expect(mockSubmit).toHaveBeenCalledWith("你好", expect.any(String)),
    );

    const activeId = mockSubmit.mock.calls[0][1];
    act(() => {
      deltaListener({ requestId: "other", delta: "不该出现" });
      deltaListener({ requestId: activeId, delta: "你" });
      deltaListener({ requestId: activeId, delta: "好" });
    });
    expect(screen.queryByText("不该出现")).not.toBeInTheDocument();
    expect(screen.getByTestId("streaming-reply")).toHaveTextContent("你好");

    act(() => completeListener({ requestId: activeId, message: "你好呀" }));
    expect(screen.getByText("你好呀")).toBeVisible();
    expect(screen.queryByTestId("streaming-reply")).not.toBeInTheDocument();

    view.unmount();
    await waitFor(() => {
      expect(unlistenDelta).toHaveBeenCalledTimes(1);
      expect(unlistenComplete).toHaveBeenCalledTimes(1);
      expect(unlistenError).toHaveBeenCalledTimes(1);
    });
  });

  it("shows a stable scoped streaming error and leaves the composer usable", async () => {
    mockSubmit.mockImplementation(async (_message, id) => ({
      kind: "chatStarted",
      requestId: id,
    }));
    render(<ChatPanel />);
    await screen.findByRole("textbox", { name: "消息" });

    fireEvent.change(screen.getByRole("textbox", { name: "消息" }), {
      target: { value: "测试" },
    });
    fireEvent.submit(screen.getByRole("form", { name: "发送消息" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));

    const activeId = mockSubmit.mock.calls[0][1];
    act(() => errorListener({ requestId: "other", code: "wrong", message: "wrong" }));
    expect(screen.queryByText(/wrong/)).not.toBeInTheDocument();
    act(() =>
      errorListener({
        requestId: activeId,
        code: "provider_unavailable",
        message: "The model provider is unavailable.",
      }),
    );

    expect(screen.getByRole("alert")).toHaveTextContent("provider_unavailable");
    fireEvent.change(screen.getByRole("textbox", { name: "消息" }), {
      target: { value: "重试" },
    });
    expect(screen.getByRole("button", { name: "发送" })).toBeEnabled();
    expect(listenChatComplete).toHaveBeenCalledTimes(1);
    expect(listenChatError).toHaveBeenCalledTimes(1);
  });
});
