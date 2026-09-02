import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ExplorationCompleteEvent,
  ExplorationErrorEvent,
  ExplorationProgressEvent,
} from "../../contracts";
import { ExplorationPanel } from "./ExplorationPanel";
import {
  cancelExploration,
  listenExplorationComplete,
  listenExplorationError,
  listenExplorationProgress,
  startExploration,
} from "../../lib/tauri";

type Listener<T> = (payload: T) => void;
let progressListener: Listener<ExplorationProgressEvent>;
let completeListener: Listener<ExplorationCompleteEvent>;
let errorListener: Listener<ExplorationErrorEvent>;
const unlisteners = [vi.fn(), vi.fn(), vi.fn()];

vi.mock("../../lib/tauri", () => ({
  cancelExploration: vi.fn(),
  startExploration: vi.fn(),
  listenExplorationProgress: vi.fn(async (listener: Listener<ExplorationProgressEvent>) => {
    progressListener = listener;
    return unlisteners[0];
  }),
  listenExplorationComplete: vi.fn(async (listener: Listener<ExplorationCompleteEvent>) => {
    completeListener = listener;
    return unlisteners[1];
  }),
  listenExplorationError: vi.fn(async (listener: Listener<ExplorationErrorEvent>) => {
    errorListener = listener;
    return unlisteners[2];
  }),
}));

describe("ExplorationPanel", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows scoped progress, supports cancel, reports errors, and cleans up listeners", async () => {
    const view = render(<ExplorationPanel taskId="task-1" />);
    await waitFor(() => expect(listenExplorationProgress).toHaveBeenCalledTimes(1));

    act(() => progressListener({ taskId: "other", status: "writing" }));
    expect(screen.queryByText("writing")).not.toBeInTheDocument();
    act(() => progressListener({ taskId: "task-1", status: "reading" }));
    expect(screen.getByTestId("exploration-progress")).toHaveTextContent("正在阅读");

    fireEvent.click(screen.getByRole("button", { name: "取消探索" }));
    expect(cancelExploration).toHaveBeenCalledWith("task-1");

    act(() =>
      errorListener({ taskId: "task-1", code: "cancelled", message: "cancelled" }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("探索已取消");

    view.unmount();
    await waitFor(() =>
      unlisteners.forEach((unlisten) => expect(unlisten).toHaveBeenCalledOnce()),
    );
  });

  it("renders exactly four results and requires a separate allow action for the next request", async () => {
    vi.mocked(startExploration).mockResolvedValue("task-2");
    render(<ExplorationPanel taskId="task-1" />);
    await waitFor(() => expect(listenExplorationComplete).toHaveBeenCalledTimes(1));

    act(() =>
      completeListener({
        taskId: "task-1",
        result: {
          items: ["甲", "乙", "丙", "丁"],
          nextOutingRequest: "我还想出去玩，可以吗？",
          rawResponse: "[omitted]",
        },
      }),
    );

    expect(screen.getAllByTestId("exploration-item")).toHaveLength(4);
    expect(screen.getByTestId("next-outing-request")).toHaveTextContent("我还想出去玩");
    expect(startExploration).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "允许出去玩" }));
    await waitFor(() =>
      expect(startExploration).toHaveBeenCalledWith("我还想出去玩，可以吗？"),
    );
    await waitFor(() => expect(listenExplorationProgress).toHaveBeenCalledTimes(2));
    expect(unlisteners[0]).toHaveBeenCalledTimes(1);

    act(() => progressListener({ taskId: "task-1", status: "writing" }));
    expect(screen.queryByText("writing")).not.toBeInTheDocument();
    act(() => progressListener({ taskId: "task-2", status: "reading" }));
    expect(screen.getByTestId("exploration-progress")).toHaveTextContent("正在阅读");
    fireEvent.click(screen.getByRole("button", { name: "取消探索" }));
    expect(cancelExploration).toHaveBeenLastCalledWith("task-2");
    expect(listenExplorationError).toHaveBeenCalledTimes(2);
  });

  it("turns provider request failures into an actionable Chinese message", async () => {
    render(<ExplorationPanel taskId="task-1" />);
    await waitFor(() => expect(listenExplorationError).toHaveBeenCalledTimes(1));

    act(() =>
      errorListener({
        taskId: "task-1",
        code: "invalid_request",
        message: "The model provider rejected the request.",
      }),
    );

    expect(screen.getByTestId("exploration-progress")).toHaveTextContent("失败");
    expect(screen.getByRole("alert")).toHaveTextContent("模型名称");
    expect(screen.getByRole("alert")).not.toHaveTextContent("model provider");
  });

  it.each([
    ["invalid_response", "模型返回的内容无法识别"],
    ["format_incomplete", "四个探索结果"],
    ["native_web_unsupported", "不支持原生联网"],
  ])("localizes %s without exposing backend text", async (code, expected) => {
    render(<ExplorationPanel taskId="task-1" />);
    await waitFor(() => expect(listenExplorationError).toHaveBeenCalledTimes(1));

    act(() =>
      errorListener({ taskId: "task-1", code, message: "provider internal detail" }),
    );

    expect(screen.getByRole("alert")).toHaveTextContent(expected);
    expect(screen.getByRole("alert")).not.toHaveTextContent("provider internal");
  });

  it("uses a fixed safe Chinese fallback for unknown exploration errors", async () => {
    render(<ExplorationPanel taskId="task-1" />);
    await waitFor(() => expect(listenExplorationError).toHaveBeenCalledTimes(1));

    act(() =>
      errorListener({
        taskId: "task-1",
        code: "unexpected_provider_error",
        message: "Authorization: Bearer sk-must-not-render",
      }),
    );

    expect(screen.getByRole("alert")).toHaveTextContent("探索暂时失败");
    expect(screen.getByRole("alert")).not.toHaveTextContent("sk-must-not-render");
  });
});
