import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatCompleteEvent,
  ChatDeltaEvent,
  ChatErrorEvent,
  ExplorationCompleteEvent,
  ExplorationDiaryDeltaEvent,
  ExplorationErrorEvent,
  ExplorationPageReadEvent,
  ExplorationProgressEvent,
  ExplorationQueryEvent,
  InputDisposition,
} from "../../contracts";
import { ChatPanel } from "./ChatPanel";
import {
  getBootstrapState,
  loadAibbProfile,
  loadChatHistory,
  loadOutingStats,
  listenChatComplete,
  listenChatDelta,
  listenChatError,
  listenChatWindowFocus,
  listenExplorationComplete,
  listenExplorationDiaryDelta,
  listenExplorationError,
  listenExplorationPageRead,
  listenExplorationProgress,
  listenExplorationQuery,
  listenProfileUpdated,
  openExternal,
  openSettingsWindow,
  submitUserInput,
} from "../../lib/tauri";

type Listener<T> = (payload: T) => void;
let deltaListener: Listener<ChatDeltaEvent>;
let completeListener: Listener<ChatCompleteEvent>;
let errorListener: Listener<ChatErrorEvent>;
let explorationProgressListener: Listener<ExplorationProgressEvent>;
let explorationCompleteListener: Listener<ExplorationCompleteEvent>;
let explorationErrorListener: Listener<ExplorationErrorEvent>;
let explorationDiaryDeltaListener: Listener<ExplorationDiaryDeltaEvent>;
let explorationQueryListener: Listener<ExplorationQueryEvent>;
let explorationPageReadListener: Listener<ExplorationPageReadEvent>;
let profileListener: Listener<{ name: string; avatarDataUrl: string | null; version: number }>;
let chatWindowFocusListener: (focused: boolean) => void;
const unlistenDelta = vi.fn();
const unlistenComplete = vi.fn();
const unlistenError = vi.fn();
const unlistenExplorationProgress = vi.fn();
const unlistenExplorationComplete = vi.fn();
const unlistenExplorationError = vi.fn();
const unlistenExplorationDiaryDelta = vi.fn();
const unlistenExplorationQuery = vi.fn();
const unlistenExplorationPageRead = vi.fn();
const unlistenProfile = vi.fn();
const unlistenChatWindowFocus = vi.fn();

vi.mock("../../lib/tauri", () => ({
  getBootstrapState: vi.fn(),
  loadAibbProfile: vi.fn(),
  loadChatHistory: vi.fn(),
  loadOutingStats: vi.fn(),
  openExternal: vi.fn(async () => {}),
  openSettingsWindow: vi.fn(),
  submitUserInput: vi.fn(),
  takePendingArchivePaths: vi.fn(async () => []),
  archiveLedger: vi.fn(async () => []),
  listenArchivePending: vi.fn(async () => () => {}),
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
  listenExplorationProgress: vi.fn(async (listener: Listener<ExplorationProgressEvent>) => {
    explorationProgressListener = listener;
    return unlistenExplorationProgress;
  }),
  listenExplorationComplete: vi.fn(async (listener: Listener<ExplorationCompleteEvent>) => {
    explorationCompleteListener = listener;
    return unlistenExplorationComplete;
  }),
  listenExplorationDiaryDelta: vi.fn(async (listener: Listener<ExplorationDiaryDeltaEvent>) => {
    explorationDiaryDeltaListener = listener;
    return unlistenExplorationDiaryDelta;
  }),
  listenExplorationQuery: vi.fn(async (listener: Listener<ExplorationQueryEvent>) => {
    explorationQueryListener = listener;
    return unlistenExplorationQuery;
  }),
  listenExplorationPageRead: vi.fn(async (listener: Listener<ExplorationPageReadEvent>) => {
    explorationPageReadListener = listener;
    return unlistenExplorationPageRead;
  }),
  listenExplorationError: vi.fn(async (listener: Listener<ExplorationErrorEvent>) => {
    explorationErrorListener = listener;
    return unlistenExplorationError;
  }),
  listenProfileUpdated: vi.fn(async (listener: typeof profileListener) => {
    profileListener = listener;
    return unlistenProfile;
  }),
  listenChatWindowFocus: vi.fn(async (listener: (focused: boolean) => void) => {
    chatWindowFocusListener = listener;
    return unlistenChatWindowFocus;
  }),
}));

const mockBootstrap = vi.mocked(getBootstrapState);
const mockSubmit = vi.mocked(submitUserInput);
const mockOpenExternal = vi.mocked(openExternal);

const diaryResult = {
  items: ["甲", "乙", "丙", "丁"] as [string, string, string, string],
  diary: "第二轮回来啦",
  sources: [
    { title: "可信来源甲", url: "https://example.com/one" },
    { title: "可信来源乙", url: "https://example.org/two" },
  ],
  images: [
    { title: "海边的日落", pageUrl: "https://example.com/sunset", dataUrl: "data:image/jpeg;base64,AQID" },
  ],
  roundNumber: 2,
  elapsedSeconds: 17,
  rawResponse: "[omitted]",
};

function deferredDisposition() {
  let resolve!: (value: InputDisposition) => void;
  const promise = new Promise<InputDisposition>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("ChatPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: null,
      version: 0,
    });
    vi.mocked(loadChatHistory).mockResolvedValue({
      messages: [],
      outings: [],
    });
    vi.mocked(loadOutingStats).mockResolvedValue({
      totalOutings: 0,
      totalDirections: 0,
      lastOutingAt: null,
      directions: [],
    });
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
    expect(screen.getByText(/platform.deepseek.com/)).toBeVisible();
    expect(screen.getByText(/不会上传到任何服务器/)).toBeVisible();
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

  it("sends with Enter while Shift+Enter and composing Enter remain in the editor", async () => {
    mockSubmit.mockImplementation(async (_message, id) => ({
      kind: "chatStarted",
      requestId: id,
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });

    fireEvent.change(editor, { target: { value: "第一行" } });
    expect(fireEvent.keyDown(editor, { key: "Enter", shiftKey: true })).toBe(true);
    expect(fireEvent.keyDown(editor, { key: "Enter", isComposing: true })).toBe(true);
    expect(mockSubmit).not.toHaveBeenCalled();

    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(false);
    await waitFor(() =>
      expect(mockSubmit).toHaveBeenCalledWith("第一行", expect.any(String)),
    );
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
      expect(unlistenExplorationProgress).toHaveBeenCalledTimes(1);
      expect(unlistenExplorationComplete).toHaveBeenCalledTimes(1);
      expect(unlistenExplorationError).toHaveBeenCalledTimes(1);
      expect(unlistenProfile).toHaveBeenCalledTimes(1);
    });
  });

  it("replaces departure progress with an outing diary in the same timeline", async () => {
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "小团子",
      avatarDataUrl: "data:image/webp;base64,AA==",
      version: 2,
    });
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-1" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await screen.findByRole("heading", { name: "小团子" });
    await waitFor(() => {
      expect(listenExplorationProgress).toHaveBeenCalledTimes(1);
      expect(listenExplorationComplete).toHaveBeenCalledTimes(1);
      expect(listenExplorationError).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(editor, { target: { value: "去玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });

    const departure = await screen.findByText("小团子 出发，去玩～");
    const outingArticle = departure.closest("article");
    expect(outingArticle).not.toBeNull();
    expect(within(outingArticle!).getByRole("img", { name: "小团子" })).toBeVisible();

    // 每个阶段都有微动画：跳动的足迹 + 三连等待点，表示动作在进行。
    expect(departure.closest(".outing-status")!.querySelector(".outing-status-paw")).not.toBeNull();
    expect(
      departure.closest(".outing-status")!.querySelectorAll(".outing-status-dots i"),
    ).toHaveLength(3);

    act(() => {
      explorationProgressListener({ taskId: "other", status: "writing" });
    });
    expect(departure).toBeVisible();
    act(() => {
      explorationProgressListener({ taskId: "task-1", status: "reading" });
    });
    const reading = screen.getByText("小团子 正在阅读～");
    expect(reading).toBeVisible();
    expect(reading.closest(".outing-status")!.querySelector(".outing-status-paw")).not.toBeNull();
    expect(
      reading.closest(".outing-status")!.querySelectorAll(".outing-status-dots i"),
    ).toHaveLength(3);

    act(() => {
      explorationCompleteListener({ taskId: "other", result: diaryResult });
    });
    expect(screen.queryByText("第二轮回来啦")).not.toBeInTheDocument();
    act(() => {
      explorationCompleteListener({ taskId: "task-1", result: diaryResult });
    });

    const diary = screen.getByText("第二轮回来啦");
    expect(diary.closest("article")).toBe(outingArticle);
    expect(within(outingArticle!).getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
    expect(within(outingArticle!).getByText("思考了 17 秒")).toBeVisible();
    expect(within(outingArticle!).getByRole("link", { name: "可信来源甲" })).toHaveAttribute(
      "href",
      "https://example.com/one",
    );
    for (const link of within(outingArticle!).getAllByRole("link")) {
      expect(link).toHaveAttribute("href");
    }
    expect(screen.queryByRole("region", { name: "探索结果" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "允许出去玩" })).not.toBeInTheDocument();
    expect(mockSubmit).toHaveBeenCalledTimes(1);
  });

  it("turns bare URLs in chat text into openable links and reports open failures", async () => {
    vi.mocked(loadChatHistory).mockResolvedValue({
      messages: [
        {
          id: "message-url",
          role: "assistant",
          content: "你看这个链接 https://example.com/abc，挺有意思的。",
          createdAt: 10,
        },
      ],
      outings: [],
    });
    render(<ChatPanel />);

    const urlLink = await screen.findByRole("link", { name: "https://example.com/abc" });
    expect(urlLink).toHaveAttribute("href", "https://example.com/abc");

    mockOpenExternal.mockRejectedValueOnce(new Error("boom"));
    fireEvent.click(urlLink);
    expect(await screen.findByTestId("link-error-toast")).toHaveTextContent("打开链接失败");
  });

  it("replays stored messages and outing diary cards when the window reopens", async () => {
    vi.mocked(loadChatHistory).mockResolvedValue({
      messages: [
        { id: "message-1", role: "user", content: "昨天问你去哪里玩", createdAt: 10 },
        {
          id: "message-2",
          role: "assistant",
          content: "去海边看日落，浪花卷着晚霞。",
          createdAt: 11,
        },
        { id: "message-3", role: "user", content: "那今天呢", createdAt: 12 },
      ],
      outings: [
        {
          roundNumber: 3,
          direction: "去海边",
          diary: "去海边看日落，浪花卷着晚霞。",
          sources: [{ title: "海边日落攻略", url: "https://example.com/sunset" }],
          images: [
            {
              title: "海边的日落",
              pageUrl: "https://example.com/sunset",
              dataUrl: "data:image/jpeg;base64,AQID",
            },
          ],
          elapsedSeconds: 21,
          createdAt: 11,
        },
      ],
    });
    render(<ChatPanel />);

    expect(await screen.findByText("昨天问你去哪里玩")).toBeVisible();
    expect(screen.getByRole("heading", { name: "第 3 轮回来啦" })).toBeVisible();
    expect(screen.getByText("思考了 21 秒")).toBeVisible();
    expect(screen.getByRole("link", { name: "海边日落攻略" })).toHaveAttribute(
      "href",
      "https://example.com/sunset",
    );
    const gallery = screen.getByLabelText("带回的图片");
    expect(within(gallery).getByRole("img", { name: "海边的日落" })).toHaveAttribute(
      "src",
      "data:image/jpeg;base64,AQID",
    );
    expect(screen.getAllByText("去海边看日落，浪花卷着晚霞。")).toHaveLength(1);
    expect(screen.getByText("那今天呢")).toBeVisible();
  });

  it("registers a focus listener so reactivating the window lands at the latest message", async () => {
    render(<ChatPanel />);
    await screen.findByRole("textbox", { name: "消息" });

    expect(listenChatWindowFocus).toHaveBeenCalledTimes(1);
    expect(chatWindowFocusListener).toBeTypeOf("function");
    act(() => {
      chatWindowFocusListener(true);
    });
    act(() => {
      chatWindowFocusListener(false);
    });
  });

  it("shows the outing trip badge and no footprint collection", async () => {
    vi.mocked(loadOutingStats).mockResolvedValue({
      totalOutings: 4,
      totalDirections: 3,
      lastOutingAt: 123456,
      directions: [
        { direction: "去看海", count: 2 },
        { direction: "去宇宙的角落", count: 1 },
        { direction: "吃遍小吃街", count: 1 },
      ],
    });
    render(<ChatPanel />);
    await screen.findByRole("textbox", { name: "消息" });

    expect(screen.getByText("🐾 ×4")).toBeVisible();
    expect(screen.queryByLabelText("出游足迹")).not.toBeInTheDocument();
  });

  it("refreshes outing stats when an outing finishes", async () => {
    mockSubmit.mockImplementation(async (_message) => ({
      kind: "explorationStarted",
      taskId: "task-1",
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去看海" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));

    const callsBefore = vi.mocked(loadOutingStats).mock.calls.length;
    act(() => explorationCompleteListener({ taskId: "task-1", result: diaryResult }));

    await waitFor(() =>
      expect(vi.mocked(loadOutingStats).mock.calls.length).toBe(callsBefore + 1),
    );
  });

  it("presents the pictures AIbb brought back as clickable cards", async () => {
    mockSubmit.mockImplementation(async (_message) => ({
      kind: "explorationStarted",
      taskId: "task-1",
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去看海" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));

    act(() => explorationCompleteListener({ taskId: "task-1", result: diaryResult }));

    const gallery = await screen.findByLabelText("带回的图片");
    const image = within(gallery).getByRole("img", { name: "海边的日落" });
    expect(image).toHaveAttribute("src", "data:image/jpeg;base64,AQID");
    expect(image.closest("a")).toHaveAttribute("href", "https://example.com/sunset");
  });

  it("shows the diary of a spontaneous outing next to the chat reply", async () => {
    mockSubmit.mockImplementation(async (_message) => ({
      kind: "chatStarted",
      requestId: "req-1",
      spontaneousTaskId: "task-spontaneous",
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "最近有什么好玩的？" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));

    expect(await screen.findByText("AIbb 偷偷溜出去玩啦～")).toBeVisible();
    act(() =>
      explorationCompleteListener({ taskId: "task-spontaneous", result: diaryResult }),
    );
    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
    expect(screen.queryByText("AIbb 偷偷溜出去玩啦～")).not.toBeInTheDocument();
  });

  it("shows a safe local message when another outing is already active", async () => {
    mockSubmit.mockRejectedValue({
      code: "exploration_already_running",
      message: "internal task details must stay hidden",
    });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });

    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "AIbb 已经在出游，请等这轮回来后再出发。",
    );
    expect(screen.queryByText(/internal task details/)).not.toBeInTheDocument();
    expect(editor).toBeEnabled();
  });

  it("keeps progress that arrives before the exploration-start response", async () => {
    const pending = deferredDisposition();
    mockSubmit.mockReturnValue(pending.promise);
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenExplorationProgress).toHaveBeenCalledTimes(1));

    fireEvent.change(editor, { target: { value: "去玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));
    act(() => explorationProgressListener({ taskId: "task-early", status: "reading" }));
    await act(async () => {
      pending.resolve({ kind: "explorationStarted", taskId: "task-early" });
      await pending.promise;
    });

    expect(screen.getByText("AIbb 正在阅读～")).toBeVisible();
    expect(screen.queryByText("AIbb 出发，去玩～")).not.toBeInTheDocument();
  });

  it("shows the chosen query and read pages while exploring, then clears them", async () => {
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-proc" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenExplorationQuery).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(listenExplorationPageRead).toHaveBeenCalledTimes(1));
    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await screen.findByText("AIbb 出发，去玩～");

    act(() => explorationQueryListener({ taskId: "task-proc", query: "深海发光生物" }));
    expect(screen.getByText("在搜：深海发光生物")).toBeVisible();

    act(() => explorationPageReadListener({
      taskId: "task-proc",
      title: "深海为什么会有光",
      url: "https://example.com/deep-sea",
    }));
    act(() => explorationPageReadListener({
      taskId: "task-proc",
      title: "会发光的鲸落",
      url: "https://example.com/whale-fall",
    }));
    expect(screen.getByText("深海为什么会有光")).toBeVisible();
    expect(screen.getByText("会发光的鲸落")).toBeVisible();

    act(() => explorationCompleteListener({ taskId: "task-proc", result: diaryResult }));
    expect(screen.queryByText("在搜：深海发光生物")).not.toBeInTheDocument();
    expect(screen.queryByText("会发光的鲸落")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
  });

  it("renders the four diary sections with distinct titles", async () => {
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-sections" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去山里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await screen.findByText("AIbb 出发，去玩～");
    act(() =>
      explorationCompleteListener({
        taskId: "task-sections",
        result: {
          ...diaryResult,
          diary:
            "## 路上的风景\n山里的雾慢慢散开。\n\n## 看图有感\n那张照片里的晚霞太美了。\n\n## 总结\n风景和照片都在说同一件事。",
        },
      }),
    );

    expect(await screen.findByRole("heading", { name: "路上的风景" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "看图有感" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "总结" })).toBeVisible();
    expect(screen.getByText("山里的雾慢慢散开。")).toBeVisible();
  });

  it("opens links in the system browser and shows a copy-only menu on right click", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-links" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await screen.findByText("AIbb 出发，去玩～");
    act(() => explorationCompleteListener({ taskId: "task-links", result: diaryResult }));

    const sourceLink = await screen.findByRole("link", { name: "可信来源甲" });
    fireEvent.click(sourceLink);
    expect(mockOpenExternal).toHaveBeenCalledWith("https://example.com/one");

    fireEvent.contextMenu(sourceLink);
    const menu = screen.getByTestId("link-context-menu");
    expect(within(menu).getByRole("menuitem", { name: /复制链接/ })).toBeVisible();
    fireEvent.click(within(menu).getByRole("menuitem", { name: /复制链接/ }));
    expect(writeText).toHaveBeenCalledWith("https://example.com/one");
    expect(screen.queryByTestId("link-context-menu")).not.toBeInTheDocument();
  });

  it("streams the diary live and replaces it with the final card", async () => {
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-live" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenExplorationDiaryDelta).toHaveBeenCalledTimes(1));
    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await screen.findByText("AIbb 出发，去玩～");

    act(() => explorationDiaryDeltaListener({ taskId: "task-live", delta: "海边的风很软，" }));
    act(() => explorationDiaryDeltaListener({ taskId: "task-live", delta: "浪花一路追着我。" }));
    expect(screen.getByText("海边的风很软，浪花一路追着我。")).toBeVisible();
    expect(screen.getByText(/正在写日记/)).toBeVisible();

    act(() => explorationCompleteListener({ taskId: "task-live", result: diaryResult }));
    expect(screen.queryByText(/正在写日记/)).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
  });

  it("keeps a completion that arrives before the exploration-start response", async () => {
    const pending = deferredDisposition();
    mockSubmit.mockReturnValue(pending.promise);
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenExplorationComplete).toHaveBeenCalledTimes(1));

    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));
    act(() => explorationCompleteListener({ taskId: "task-early", result: diaryResult }));
    await act(async () => {
      pending.resolve({ kind: "explorationStarted", taskId: "task-early" });
      await pending.promise;
    });

    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
    expect(screen.queryByText("AIbb 出发，去玩～")).not.toBeInTheDocument();
  });

  it("keeps an error that arrives before the exploration-start response", async () => {
    const pending = deferredDisposition();
    mockSubmit.mockReturnValue(pending.promise);
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenExplorationError).toHaveBeenCalledTimes(1));

    fireEvent.change(editor, { target: { value: "去玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));
    act(() => explorationErrorListener({
      taskId: "task-early",
      code: "provider_unavailable",
      message: "provider detail",
    }));
    await act(async () => {
      pending.resolve({ kind: "explorationStarted", taskId: "task-early" });
      await pending.promise;
    });

    expect(screen.getByText("模型服务暂时不可用，请稍后重试。")).toBeVisible();
    expect(screen.queryByText("AIbb 出发，去玩～")).not.toBeInTheDocument();
  });

  it("keeps the first terminal outing event when a conflicting event arrives late", async () => {
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-terminal" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await screen.findByText("AIbb 出发，去玩～");

    act(() => explorationCompleteListener({ taskId: "task-terminal", result: diaryResult }));
    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();

    act(() => explorationErrorListener({
      taskId: "task-terminal",
      code: "provider_unavailable",
      message: "late conflicting terminal event",
    }));

    expect(screen.getByRole("heading", { name: "第 2 轮回来啦" })).toBeVisible();
    expect(screen.queryByText("模型服务暂时不可用，请稍后重试。")).not.toBeInTheDocument();
  });

  it("replaces an outing placeholder with a safe Chinese error", async () => {
    mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-1" });
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "去海里玩" } });
    fireEvent.keyDown(editor, { key: "Enter" });

    const departure = await screen.findByText("AIbb 出发，去玩～");
    const outingArticle = departure.closest("article");
    act(() => {
      explorationErrorListener({
        taskId: "task-1",
        code: "invalid_request",
        message: "Authorization: Bearer sk-must-not-render",
      });
    });

    const safeError = screen.getByText(/请求被模型服务拒绝/);
    expect(safeError.closest("article")).toBe(outingArticle);
    expect(screen.queryByText(/sk-must-not-render/)).not.toBeInTheDocument();
    expect(editor).toBeEnabled();
  });

  it("keeps an ordinary message as an ordinary chat reply", async () => {
    mockSubmit.mockImplementation(async (_message, id) => ({
      kind: "chatStarted",
      requestId: id,
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });

    fireEvent.change(editor, { target: { value: "今天星期几？" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));
    const activeId = mockSubmit.mock.calls[0][1];
    act(() => completeListener({ requestId: activeId, message: "今天是星期四。" }));

    expect(screen.getByText("今天是星期四。")).toBeVisible();
    expect(screen.queryByText(/出发，去玩/)).not.toBeInTheDocument();
    expect(screen.queryByText(/轮回来啦/)).not.toBeInTheDocument();
  });

  it("renames existing assistant messages after a profile update", async () => {
    mockSubmit.mockImplementation(async (_message, id) => ({
      kind: "chatStarted",
      requestId: id,
    }));
    render(<ChatPanel />);
    const editor = await screen.findByRole("textbox", { name: "消息" });
    fireEvent.change(editor, { target: { value: "你好" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(1));

    act(() => completeListener({ requestId: mockSubmit.mock.calls[0][1], message: "旧消息正文" }));
    fireEvent.change(editor, { target: { value: "第二条消息" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(mockSubmit).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    act(() => profileListener({
      name: "小团子",
      avatarDataUrl: "data:image/webp;base64,AA==",
      version: 2,
    }));

    expect(screen.getByRole("heading", { name: "小团子" })).toBeVisible();
    const historicalArticle = screen.getByText("旧消息正文").closest("article");
    const activeArticle = screen.getByTestId("streaming-reply").closest("article");
    expect(historicalArticle).not.toBeNull();
    expect(activeArticle).not.toBeNull();
    expect(within(historicalArticle!).getByRole("img", { name: "小团子" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AA==",
    );
    expect(within(historicalArticle!).getByText("小团子", { selector: ".message-author" })).toBeVisible();
    expect(within(activeArticle!).getByRole("img", { name: "小团子" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AA==",
    );
    expect(within(activeArticle!).getByText("小团子", { selector: ".message-author" })).toBeVisible();
    expect(screen.getByText("旧消息正文")).toBeVisible();
  });

  it("cleans up a resolved profile listener when another listener rejects", async () => {
    vi.mocked(listenChatDelta).mockRejectedValueOnce(new Error("registration failed"));
    const view = render(<ChatPanel />);

    await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    view.unmount();

    await waitFor(() => expect(unlistenProfile).toHaveBeenCalledTimes(1));
  });

  it("keeps the default identity when profile loading fails", async () => {
    vi.mocked(loadAibbProfile).mockRejectedValueOnce(new Error("profile unavailable"));
    render(<ChatPanel />);

    await screen.findByRole("textbox", { name: "消息" });
    await waitFor(() => expect(loadAibbProfile).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("heading", { name: "AIbb" })).toBeVisible();
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
