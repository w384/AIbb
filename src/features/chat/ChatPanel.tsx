import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
} from "react";
import { AibbAvatar } from "../../components/AibbAvatar";
import { ExternalLink } from "../../components/ExternalLink";
import { ArchivePanel } from "../archive/ArchivePanel";
import type {
  AibbProfile,
  AppErrorPayload,
  BootstrapState,
  ChatHistory,
  CompletedOuting,
  ExplorationResult,
  ExplorationStatus,
  OutingStats,
  OutingTimelineMessage,
} from "../../contracts";
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
  openSettingsWindow,
  submitUserInput,
} from "../../lib/tauri";

const FIRST_RUN_GREETING =
  "你好！我是喜欢出去玩耍的快乐 AIbb。先配置一个大模型 API，我们再聊天吧。";
const DEFAULT_PROFILE: AibbProfile = {
  name: "AIbb",
  avatarDataUrl: null,
  version: 0,
};

interface ChatMessageView {
  id: string;
  role: "user" | "assistant";
  kind: "text";
  content: string;
}

type TimelineMessage = ChatMessageView | OutingTimelineMessage;

function requestId(): string {
  return globalThis.crypto?.randomUUID?.() ??
    `request-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function publicError(error: unknown): AppErrorPayload {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string"
  ) {
    const code = error.code;
    return {
      code,
      message: publicExplorationError({ code, message: "" }),
    };
  }
  return { code: "request_failed", message: "操作失败。" };
}

function publicExplorationError(error: AppErrorPayload): string {
  const messages: Record<string, string> = {
    invalidSettings: "请打开设置，填写并保存 HTTPS API 地址和模型名称后重试。",
    invalid_request: "请求被模型服务拒绝，请打开设置确认模型名称已经保存。",
    model_not_found: "找不到当前模型，请打开设置检查模型名称。",
    authentication_failed: "API Key 认证失败，请打开设置重新检查。",
    rate_limited: "请求过于频繁，请稍后再让 AIbb 出去玩。",
    request_timeout: "探索请求超时，请稍后重试。",
    provider_unavailable: "模型服务暂时不可用，请稍后重试。",
    invalid_response: "模型返回的内容无法识别，请稍后重试。",
    invalid_outing_diary: "这次出游日记没有整理好，请稍后重试。",
    missing_outing_sources: "这次没有找到可信来源，AIbb 已安全返回。",
    exploration_already_running: "AIbb 已经在出游，请等这轮回来后再出发。",
    cancelled: "探索已取消。",
    unsafe_url: "探索遇到了不安全的网页地址，已停止访问。",
    unsupported_content: "探索页面的内容格式暂不支持。",
    response_too_large: "探索页面内容过大，AIbb 已停止读取。",
    public_search_unavailable: "公共搜索暂时不可用，请稍后重试。",
    public_page_unavailable: "探索页面暂时无法访问，请稍后重试。",
    redirect_limit_exceeded: "探索页面跳转次数过多，已停止访问。",
    page_budget_exceeded: "本次探索读取的页面已达到上限。",
    provider_capability_unsupported: "当前模型不支持这项联网能力。",
    native_web_unsupported: "当前模型不支持原生联网，请在设置中使用自动探测。",
    format_incomplete: "模型没有返回完整的四个探索结果，请稍后重试。",
    invalid_query_envelope: "模型生成的搜索方向无法识别，请稍后重试。",
    exploration_storage_unavailable: "探索记录暂时无法保存，请稍后重试。",
    exploration_state_unavailable: "探索状态暂时不可用，请稍后重试。",
    exploration_not_found: "没有找到这次探索记录。",
    exploration_not_cancellable: "这次探索已经结束，无法再取消。",
    invalid_exploration_transition: "探索状态出现异常，请重新开始。",
    storageUnavailable: "本地数据暂时无法访问，请重启 AIbb 后重试。",
  };
  return messages[error.code] ?? "探索暂时失败，请稍后重试；若持续发生，请检查模型设置。";
}

function explorationStatusLabel(status: ExplorationStatus): string {
  const labels: Record<ExplorationStatus, string> = {
    queued: "准备出发",
    choosing: "正在决定方向",
    nativeSearching: "正在联网探索",
    publicSearching: "正在搜索公开内容",
    reading: "正在阅读",
    writing: "正在整理发现",
    correcting: "正在完善结果",
    completed: "探索完成",
    cancelled: "已取消",
    interrupted: "已中断",
    failed: "失败",
  };
  return labels[status];
}

/**
 * Splits free text into plain segments and bare http(s) URLs so the renderer
 * can turn the URLs into clickable links (the model often mentions a link in
 * plain chat text). Trailing punctuation is kept out of the URL.
 */
function linkify(text: string): Array<{ text: string; url: string | null }> {
  const parts: Array<{ text: string; url: string | null }> = [];
  const pattern = /https?:\/\/[^\s<>"'`，。！？；：、）】》」…]+/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) {
      parts.push({ text: text.slice(last, index), url: null });
    }
    parts.push({ text: match[0], url: match[0] });
    last = index + match[0].length;
  }
  if (last < text.length) {
    parts.push({ text: text.slice(last), url: null });
  }
  return parts;
}

/** Plain chat text with any bare URLs rendered as openable links. */
function MessageText({
  content,
  onLinkContextMenu,
  onOpenError,
}: {
  content: string;
  onLinkContextMenu: (event: MouseEvent, url: string) => void;
  onOpenError: (message: string) => void;
}) {
  return (
    <p className="message">
      {linkify(content).map((part, index) =>
        part.url ? (
          <ExternalLink
            key={index}
            href={part.url}
            onLinkContextMenu={onLinkContextMenu}
            onOpenError={onOpenError}
          >
            {part.text}
          </ExternalLink>
        ) : (
          <Fragment key={index}>{part.text}</Fragment>
        ),
      )}
    </p>
  );
}

function outingDiaryMessage(
  taskId: string,
  result: ExplorationResult,
): OutingTimelineMessage {
  return {
    id: `outing-${taskId}`,
    role: "assistant",
    kind: "outingDiary",
    taskId,
    content: result.diary,
    sources: result.sources,
    images: result.images,
    roundNumber: result.roundNumber,
    elapsedSeconds: result.elapsedSeconds,
    highlights: result.highlights ?? [],
  };
}

function historyDiaryMessage(outing: CompletedOuting): OutingTimelineMessage {
  return {
    id: `outing-history-${outing.createdAt}`,
    role: "assistant",
    kind: "outingDiary",
    taskId: `history-${outing.createdAt}`,
    content: outing.diary,
    sources: outing.sources,
    images: outing.images,
    roundNumber: outing.roundNumber,
    elapsedSeconds: outing.elapsedSeconds,
  };
}

/**
 * Rebuilds the stored conversation into one chronological timeline: text
 * messages in place, and finished outings as diary cards. The plain diary
 * text that the store also writes into `messages` is replaced by its card so
 * it is not shown twice.
 */
function replayHistory(history: ChatHistory): TimelineMessage[] {
  const unclaimed = new Map<string, CompletedOuting>();
  for (const outing of history.outings) {
    unclaimed.set(outing.diary, outing);
  }
  const timeline: Array<{ createdAt: number; message: TimelineMessage }> = [];
  for (const message of history.messages) {
    const outing =
      message.role === "assistant" ? unclaimed.get(message.content) : undefined;
    if (outing) {
      unclaimed.delete(message.content);
      timeline.push({ createdAt: outing.createdAt, message: historyDiaryMessage(outing) });
      continue;
    }
    timeline.push({
      createdAt: message.createdAt,
      message: {
        id: message.id,
        role: message.role === "assistant" ? "assistant" : "user",
        kind: "text",
        content: message.content,
      },
    });
  }
  for (const outing of unclaimed.values()) {
    timeline.push({ createdAt: outing.createdAt, message: historyDiaryMessage(outing) });
  }
  timeline.sort((left, right) => left.createdAt - right.createdAt);
  return timeline.map((entry) => entry.message);
}

function replaceOutingMessage(
  messages: TimelineMessage[],
  taskId: string,
  replacement: OutingTimelineMessage,
): TimelineMessage[] {
  const index = messages.findIndex(
    (message) => message.kind !== "text" && message.taskId === taskId,
  );
  if (index < 0) return [...messages, replacement];
  const existing = messages[index];
  if (existing.kind === "text") return messages;
  const newer = newerOutingMessage(existing, replacement);
  if (newer === existing) return messages;
  return messages.map((message, messageIndex) =>
    messageIndex === index ? newer : message,
  );
}

function newerOutingMessage(
  current: OutingTimelineMessage | undefined,
  incoming: OutingTimelineMessage,
): OutingTimelineMessage {
  if (current && current.kind !== "outingStatus") return current;
  return incoming;
}

function ChatHeader({
  profile,
  totalOutings,
}: {
  profile: AibbProfile;
  totalOutings: number | null;
}) {
  return (
    <header className="chat-header">
      <span className="chat-avatar">
        <AibbAvatar avatarDataUrl={profile.avatarDataUrl} name={profile.name} />
      </span>
      <div className="chat-identity">
        <h1>{profile.name}</h1>
        <p>
          <span className="online-dot" aria-hidden="true" />准备出去玩
          {totalOutings !== null && totalOutings > 0 && (
            <span
              className="outing-trip-badge"
              title={`AIbb 已经出去玩过 ${totalOutings} 次`}
            >
              🐾 ×{totalOutings}
            </span>
          )}
        </p>
      </div>
      <button
        aria-label="打开设置"
        className="chat-settings-button"
        type="button"
        onClick={() => void openSettingsWindow()}
      >
        <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24">
          <path d="M12 8.25A3.75 3.75 0 1 0 12 15.75 3.75 3.75 0 0 0 12 8.25Zm8 3.75-2.05-.78a6.3 6.3 0 0 0-.58-1.4l.9-2-2.1-2.1-2 .9a6.3 6.3 0 0 0-1.4-.58L12 4h-3l-.78 2.05a6.3 6.3 0 0 0-1.4.58l-2-.9-2.1 2.1.9 2a6.3 6.3 0 0 0-.58 1.4L1 12v3l2.05.78c.14.49.34.96.58 1.4l-.9 2 2.1 2.1 2-.9c.44.24.91.44 1.4.58L9 23h3l.78-2.05a6.3 6.3 0 0 0 1.4-.58l2 .9 2.1-2.1-.9-2c.24-.44.44-.91.58-1.4L20 15v-3Z" />
        </svg>
      </button>
    </header>
  );
}

function AssistantIdentity({ profile }: { profile: AibbProfile }) {
  return (
    <div className="message-identity">
      <span className="message-avatar">
        <AibbAvatar avatarDataUrl={profile.avatarDataUrl} name={profile.name} />
      </span>
      <span className="message-author">{profile.name}</span>
    </div>
  );
}

/** Splits one diary block into an optional `## section title` and its body. */
function parseDiaryBlock(block: string): { title: string | null; body: string } {
  const lines = block.split("\n");
  const titleMatch = lines[0]?.trim().match(/^##\s+(.+)$/);
  if (titleMatch) {
    return { title: titleMatch[1].trim(), body: lines.slice(1).join("\n").trim() };
  }
  return { title: null, body: block.trim() };
}

function DiaryBody({
  message,
  onLinkContextMenu,
  onOpenError,
}: {
  message: Extract<OutingTimelineMessage, { kind: "outingDiary" }>;
  onLinkContextMenu: (event: MouseEvent, url: string) => void;
  onOpenError: (message: string) => void;
}) {
  const paragraphs = message.content
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.trim())
    .filter(Boolean);
  const highlights = message.highlights ?? [];
  return (
    <>
      {paragraphs.map((block, index) => {
        const part = parseDiaryBlock(block);
        const highlight = highlights.find((spot) => spot.paragraph === index);
        return (
          <Fragment key={index}>
            {part.title && (
              <h3 className="outing-diary-section-title">{part.title}</h3>
            )}
            {part.body && <p className="outing-diary-paragraph">{part.body}</p>}
            {highlight && (
              <div className="outing-diary-highlight" aria-label="分享的内容">
                {highlight.imageIndex != null &&
                  message.images[highlight.imageIndex] && (
                    <ExternalLink
                      className="outing-image"
                      href={message.images[highlight.imageIndex].pageUrl}
                      title={message.images[highlight.imageIndex].title}
                      onLinkContextMenu={onLinkContextMenu}
                      onOpenError={onOpenError}
                    >
                      <img
                        src={message.images[highlight.imageIndex].dataUrl}
                        alt={message.images[highlight.imageIndex].title}
                        loading="lazy"
                      />
                    </ExternalLink>
                  )}
                {highlight.sourceIndex != null &&
                  message.sources[highlight.sourceIndex] && (
                    <ExternalLink
                      className="outing-source-link"
                      href={message.sources[highlight.sourceIndex].url}
                      onLinkContextMenu={onLinkContextMenu}
                      onOpenError={onOpenError}
                    >
                      🔗 {message.sources[highlight.sourceIndex].title}
                    </ExternalLink>
                  )}
              </div>
            )}
          </Fragment>
        );
      })}
    </>
  );
}

function MessageBody({
  message,
  profile,
  onLinkContextMenu,
  onOpenError,
}: {
  message: TimelineMessage;
  profile: AibbProfile;
  onLinkContextMenu: (event: MouseEvent, url: string) => void;
  onOpenError: (message: string) => void;
}) {
  if (message.kind === "outingDiary") {
    return (
      <div className="message outing-diary">
        <div className="outing-diary-heading">
          <h2>第 {message.roundNumber} 轮回来啦</h2>
          <span>思考了 {message.elapsedSeconds} 秒</span>
        </div>
        <DiaryBody message={message} onLinkContextMenu={onLinkContextMenu} onOpenError={onOpenError} />
        {message.images.length > 0 && (
          <div className="outing-images" aria-label="带回的图片">
            {message.images.map((image) => (
              <ExternalLink
                className="outing-image"
                href={image.pageUrl}
                key={image.dataUrl}
                title={image.title}
                onLinkContextMenu={onLinkContextMenu}
                onOpenError={onOpenError}
              >
                <img src={image.dataUrl} alt={image.title} loading="lazy" />
              </ExternalLink>
            ))}
          </div>
        )}
        {message.sources.length > 0 && (
          <ul className="outing-sources" aria-label="来源">
            {message.sources.map((source) => (
              <li key={source.url}>
                <ExternalLink
                  href={source.url}
                  onLinkContextMenu={onLinkContextMenu}
                  onOpenError={onOpenError}
                >
                  {source.title}
                </ExternalLink>
              </li>
            ))}
          </ul>
        )}
      </div>
    );
  }
  if (message.kind === "outingStatus") {
    return (
      <p className="message outing-status">
        <span className="outing-status-paw" aria-hidden="true">🐾</span>
        <span className="outing-status-text">
          {profile.name} {message.content}
        </span>
        <span className="outing-status-dots" aria-hidden="true">
          <i /><i /><i />
        </span>
      </p>
    );
  }
  if (message.kind === "outingError") {
    return <p className="message outing-error" role="alert">{message.content}</p>;
  }
  return (
    <MessageText
      content={message.content}
      onLinkContextMenu={onLinkContextMenu}
      onOpenError={onOpenError}
    />
  );
}

export function ChatPanel() {
  const [bootstrap, setBootstrap] = useState<BootstrapState | null>(null);
  const [profile, setProfile] = useState<AibbProfile>(DEFAULT_PROFILE);
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<TimelineMessage[]>([]);
  const [streamingReply, setStreamingReply] = useState("");
  const [activeRequestId, setActiveRequestId] = useState<string | null>(null);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const [outingStats, setOutingStats] = useState<OutingStats | null>(null);
  const [liveDiaries, setLiveDiaries] = useState<Record<string, string>>({});
  const [outingProcess, setOutingProcess] = useState<
    Record<string, { query: string | null; pages: Array<{ title: string; url: string }> }>
  >({});
  const [linkMenu, setLinkMenu] = useState<{ x: number; y: number; url: string } | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);

  useEffect(() => {
    if (!linkError) return;
    const timer = window.setTimeout(() => setLinkError(null), 3500);
    return () => window.clearTimeout(timer);
  }, [linkError]);

  const openLinkMenu = (event: { clientX: number; clientY: number }, url: string) => {
    setLinkMenu({ x: event.clientX, y: event.clientY, url });
  };

  // Capture-phase fallback: on some WebView2 builds React's synthetic
  // onContextMenu never fires, so grab the event at the document root before
  // the native menu can appear and route it to the same link menu.
  const openLinkMenuRef = useRef(openLinkMenu);
  openLinkMenuRef.current = openLinkMenu;
  useEffect(() => {
    const onContextMenu = (event: globalThis.MouseEvent) => {
      const target = event.target as HTMLElement | null;
      const link = target?.closest?.("a[href]");
      if (!link) return;
      event.preventDefault();
      openLinkMenuRef.current(event, link.getAttribute("href") ?? "");
    };
    document.addEventListener("contextmenu", onContextMenu, true);
    return () => document.removeEventListener("contextmenu", onContextMenu, true);
  }, []);

  const copyText = async (text: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
        return;
      }
    } catch {
      // fall through to the legacy path
    }
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand("copy");
    document.body.removeChild(textarea);
  };

  useEffect(() => {
    if (!linkMenu) return;
    const closeMenu = () => setLinkMenu(null);
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenu();
    };
    document.addEventListener("click", closeMenu);
    document.addEventListener("contextmenu", closeMenu);
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("scroll", closeMenu, true);
    return () => {
      document.removeEventListener("click", closeMenu);
      document.removeEventListener("contextmenu", closeMenu);
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("scroll", closeMenu, true);
    };
  }, [linkMenu]);
  const activeRequest = useRef<string | null>(null);
  const knownOutings = useRef(new Set<string>());
  const earlyOutingEvents = useRef(new Map<string, OutingTimelineMessage>());
  const conversationRef = useRef<HTMLElement | null>(null);
  const stickToBottomRef = useRef(true);

  const scrollToLatest = () => {
    const container = conversationRef.current;
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  };

  const handleConversationScroll = () => {
    const container = conversationRef.current;
    if (!container) return;
    const distance =
      container.scrollHeight - container.scrollTop - container.clientHeight;
    stickToBottomRef.current = distance < 80;
  };

  useEffect(() => {
    if (stickToBottomRef.current) scrollToLatest();
  });

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const installListener = (registration: Promise<() => void>) => {
      void registration
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlisteners.push(unlisten);
        })
        .catch(() => {});
    };
    const refreshOutingStats = () => {
      void loadOutingStats()
        .then((stats) => {
          if (!disposed) setOutingStats(stats);
        })
        .catch(() => {});
    };
    refreshOutingStats();
    installListener(
      listenChatWindowFocus((focused) => {
        if (!focused || disposed) return;
        stickToBottomRef.current = true;
        scrollToLatest();
      }),
    );
    installListener(listenProfileUpdated((updatedProfile) => {
      setProfile((current) =>
        updatedProfile.version >= current.version ? updatedProfile : current,
      );
    }));
    installListener(listenChatDelta((event) => {
      if (event.requestId === activeRequest.current) {
        setStreamingReply((reply) => reply + event.delta);
      }
    }));
    installListener(listenChatComplete((event) => {
      if (event.requestId !== activeRequest.current) return;
      setMessages((current) => [
        ...current,
        { id: `assistant-${event.requestId}`, role: "assistant", kind: "text", content: event.message },
      ]);
      activeRequest.current = null;
      setActiveRequestId(null);
      setStreamingReply("");
    }));
    const receiveOutingEvent = (taskId: string, incoming: OutingTimelineMessage) => {
      if (!knownOutings.current.has(taskId)) {
        earlyOutingEvents.current.set(
          taskId,
          newerOutingMessage(earlyOutingEvents.current.get(taskId), incoming),
        );
        if (earlyOutingEvents.current.size > 32) {
          const oldest = earlyOutingEvents.current.keys().next().value;
          if (oldest) earlyOutingEvents.current.delete(oldest);
        }
        return;
      }
      setMessages((current) =>
        replaceOutingMessage(current, taskId, incoming),
      );
    };
    installListener(listenExplorationProgress((event) => {
      receiveOutingEvent(event.taskId, {
        id: `outing-${event.taskId}`,
        role: "assistant",
        kind: "outingStatus",
        taskId: event.taskId,
        content: `${explorationStatusLabel(event.status)}～`,
      });
    }));
    installListener(listenExplorationQuery((event) => {
      setOutingProcess((current) => ({
        ...current,
        [event.taskId]: {
          query: event.query,
          pages: current[event.taskId]?.pages ?? [],
        },
      }));
    }));
    installListener(listenExplorationPageRead((event) => {
      setOutingProcess((current) => ({
        ...current,
        [event.taskId]: {
          query: current[event.taskId]?.query ?? null,
          pages: [...(current[event.taskId]?.pages ?? []), { title: event.title, url: event.url }],
        },
      }));
    }));
    const clearOutingProcess = (taskId: string) => {
      setOutingProcess((current) => {
        const { [taskId]: _dropped, ...rest } = current;
        return rest;
      });
    };
    installListener(listenExplorationDiaryDelta((event) => {
      setLiveDiaries((current) => ({
        ...current,
        [event.taskId]: (current[event.taskId] ?? "") + event.delta,
      }));
    }));
    installListener(listenExplorationComplete((event) => {
      setLiveDiaries((current) => {
        const { [event.taskId]: _dropped, ...rest } = current;
        return rest;
      });
      clearOutingProcess(event.taskId);
      receiveOutingEvent(
        event.taskId,
        outingDiaryMessage(event.taskId, event.result),
      );
      refreshOutingStats();
    }));
    installListener(listenExplorationError((event) => {
      setLiveDiaries((current) => {
        const { [event.taskId]: _dropped, ...rest } = current;
        return rest;
      });
      clearOutingProcess(event.taskId);
      receiveOutingEvent(event.taskId, {
        id: `outing-${event.taskId}`,
        role: "assistant",
        kind: "outingError",
        taskId: event.taskId,
        content: publicExplorationError(event),
      });
    }));
    installListener(listenChatError((event) => {
      if (event.requestId !== activeRequest.current) return;
      setError({ code: event.code, message: event.message });
      activeRequest.current = null;
      setActiveRequestId(null);
      setStreamingReply("");
    }));
    void loadChatHistory()
      .then((history) => {
        if (disposed) return;
        const replay = replayHistory(history);
        if (replay.length > 0) {
          setMessages((current) => (current.length === 0 ? replay : current));
        }
      })
      .catch(() => {});
    void loadAibbProfile()
      .then((loadedProfile) => {
        if (!disposed) {
          setProfile((current) =>
            loadedProfile.version >= current.version ? loadedProfile : current,
          );
        }
      })
      .catch(() => {});
    void getBootstrapState()
      .then((state) => {
        if (!disposed) setBootstrap(state);
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(publicError(reason));
      });
    return () => {
      disposed = true;
      earlyOutingEvents.current.clear();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  async function sendCurrentInput() {
    const message = input.trim();
    if (!message || activeRequest.current) return;
    const id = requestId();
    activeRequest.current = id;
    setActiveRequestId(id);
    setError(null);
    setInput("");
    setStreamingReply("");
    setMessages((current) => [
      ...current,
      { id: `user-${id}`, role: "user", kind: "text", content: message },
    ]);
    try {
      const disposition = await submitUserInput(message, id);
      if (disposition.kind === "explorationStarted") {
        activeRequest.current = null;
        setActiveRequestId(null);
        attachOuting(disposition.taskId, "出发，去玩～");
      } else if (disposition.spontaneousTaskId) {
        attachOuting(disposition.spontaneousTaskId, "偷偷溜出去玩啦～");
      }
    } catch (reason) {
      activeRequest.current = null;
      setActiveRequestId(null);
      setError(publicError(reason));
    }
  }

  function attachOuting(taskId: string, placeholder: string) {
    knownOutings.current.add(taskId);
    const earlyEvent = earlyOutingEvents.current.get(taskId);
    earlyOutingEvents.current.delete(taskId);
    setMessages((current) =>
      replaceOutingMessage(current, taskId, earlyEvent ?? {
        id: `outing-${taskId}`,
        role: "assistant",
        kind: "outingStatus",
        taskId,
        content: placeholder,
      }),
    );
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void sendCurrentInput();
  }

  function handleEditorKeyDown(event: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key !== "Enter" ||
      event.shiftKey ||
      event.nativeEvent.isComposing
    ) {
      return;
    }
    event.preventDefault();
    void sendCurrentInput();
  }

  if (!bootstrap && !error) {
    return <main className="panel chat-panel">正在唤醒 {profile.name}…</main>;
  }
  if (bootstrap && !bootstrap.apiConfigured) {
    return (
      <main className="panel chat-panel" aria-label={`${profile.name} 聊天`}>
        <ChatHeader profile={profile} totalOutings={outingStats?.totalOutings ?? null} />
        <section className="first-run-card" aria-label="快速上手">
          <span className="first-run-sparkle" aria-hidden="true">✦</span>
          <h2>你好呀！</h2>
          <p>{FIRST_RUN_GREETING}</p>
          <h3 className="first-run-subhead">三步连上我</h3>
          <ol className="first-run-steps">
            <li>
              <strong>准备 Key</strong>：去{" "}
              <ExternalLink href="https://platform.deepseek.com" onLinkContextMenu={openLinkMenu}>platform.deepseek.com</ExternalLink>{" "}
              注册并创建 API Key（也支持其他兼容 OpenAI 的服务）。
            </li>
            <li>
              <strong>粘贴并测试</strong>：点下面的按钮打开设置，粘贴 Key——API 地址和模型会自动填好，再点「保存并测试」。
            </li>
            <li>
              <strong>开始玩</strong>：提示连接成功后，回到对话窗跟我说「出去玩」就行。
            </li>
          </ol>
          <h3 className="first-run-subhead">怎么跟我玩</h3>
          <ul className="first-run-tips">
            <li><strong>点我</strong>：打开对话窗，随时聊天。</li>
            <li><strong>长按拖动</strong>：移动悬浮窗，靠近屏幕边缘会自动吸附。</li>
            <li><strong>右键我</strong>：打开设置。</li>
            <li><strong>拖文件到我身上</strong>：归档进本地资料库。</li>
            <li>跟我说「去海边玩」「出去逛逛」，我会自己去网上找新鲜事带回来。</li>
          </ul>
          <p className="first-run-privacy">
            🔒 API Key 只保存在这台电脑的系统凭据里，对话与记忆也不会上传到任何服务器。
          </p>
          <button className="chat-primary-button" type="button" onClick={() => void openSettingsWindow()}>
            打开 API 设置
          </button>
        </section>
        {error && (
          <p className="chat-feedback" role="alert">
            <span className="chat-error-code">{error.code}</span>
            <span>{error.message}</span>
          </p>
        )}
        <ArchivePanel />
      </main>
    );
  }

  return (
    <main className="panel chat-panel" aria-label={`${profile.name} 聊天`}>
      <ChatHeader profile={profile} totalOutings={outingStats?.totalOutings ?? null} />
      <section
        className="conversation"
        aria-live="polite"
        ref={conversationRef}
        onScroll={handleConversationScroll}
      >
        {messages.length === 0 && !activeRequestId && (
          <div className="chat-welcome">
            <span aria-hidden="true">✦</span>
            <h2>今天想去哪里玩？</h2>
            <p>告诉我一个方向，或者让我自己决定。</p>
          </div>
        )}
        {messages.map((message) => (
          <article key={message.id} className={`message-row ${message.role}`}>
            {message.role === "assistant" ? <AssistantIdentity profile={profile} /> : <span className="message-author">你</span>}
            <MessageBody
              message={message}
              profile={profile}
              onLinkContextMenu={openLinkMenu}
              onOpenError={setLinkError}
            />
          </article>
        ))}
        {Object.entries(outingProcess).map(([taskId, process]) =>
          process.query || process.pages.length > 0 ? (
            <article key={`process-${taskId}`} className="message-row assistant">
              <AssistantIdentity profile={profile} />
              <div className="message outing-process">
                {process.query && (
                  <p className="outing-process-line">
                    <span aria-hidden="true">🔎</span>
                    在搜：{process.query}
                  </p>
                )}
                {process.pages.length > 0 && (
                  <div className="outing-process-pages">
                    <p className="outing-process-line">
                      <span aria-hidden="true">📖</span>
                      {profile.name} 读到了：
                    </p>
                    <ul>
                      {process.pages.map((page, index) => (
                        <li key={`${page.url}-${index}`}>
                          <ExternalLink href={page.url} onLinkContextMenu={openLinkMenu}>
                            {page.title}
                          </ExternalLink>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            </article>
          ) : null,
        )}
        {Object.entries(liveDiaries).map(([taskId, text]) => (
          <article key={`live-${taskId}`} className="message-row assistant">
            <AssistantIdentity profile={profile} />
            <div className="message outing-diary outing-diary-live">
              <div className="outing-diary-heading">
                <h2>✍️ {profile.name} 正在写日记…</h2>
                <span>直播中</span>
              </div>
              <div className="outing-diary-content" data-testid={`live-diary-${taskId}`}>
                {text ? (
                  text
                    .split(/\n\s*\n/)
                    .map((block) => block.trim())
                    .filter(Boolean)
                    .map((block, index) => {
                      const part = parseDiaryBlock(block);
                      return (
                        <Fragment key={index}>
                          {part.title && (
                            <h3 className="outing-diary-section-title">{part.title}</h3>
                          )}
                          {part.body && (
                            <p className="outing-diary-paragraph">{part.body}</p>
                          )}
                        </Fragment>
                      );
                    })
                ) : (
                  <>
                    <span className="thinking-dot" />
                    起笔了…
                  </>
                )}
              </div>
            </div>
          </article>
        ))}
        {activeRequestId && (
          <article className="message-row assistant">
            <AssistantIdentity profile={profile} />
            <p className="message thinking" data-testid="streaming-reply">
              {streamingReply || <><span className="thinking-dot" />正在想…</>}
            </p>
          </article>
        )}
      </section>
      {error && (
        <p className="chat-feedback" role="alert">
          <span className="chat-error-code">{error.code}</span>
          <span>{error.message}</span>
        </p>
      )}
      <ArchivePanel />
      <form aria-label="发送消息" className="composer" onSubmit={submit}>
        <label>
          <span className="sr-only">消息</span>
          <textarea
            aria-label="消息"
            placeholder={`和 ${profile.name} 说点什么…`}
            rows={1}
            value={input}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={handleEditorKeyDown}
          />
        </label>
        <button className="send-button" type="submit" disabled={Boolean(activeRequestId) || !input.trim()}>
          发送
        </button>
      </form>
      {linkError && (
        <p className="link-error-toast" role="alert" data-testid="link-error-toast">
          打开链接失败：{linkError}
        </p>
      )}
      {linkMenu && (
        <div
          className="link-context-menu"
          style={{ left: linkMenu.x, top: linkMenu.y }}
          role="menu"
          aria-label="链接菜单"
          data-testid="link-context-menu"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              void copyText(linkMenu.url);
              setLinkMenu(null);
            }}
          >
            🔗 复制链接
          </button>
        </div>
      )}
    </main>
  );
}
