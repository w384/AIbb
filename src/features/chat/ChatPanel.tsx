import { useEffect, useRef, useState, type FormEvent } from "react";
import { AibbAvatar } from "../../components/AibbAvatar";
import type { AppErrorPayload, BootstrapState } from "../../contracts";
import {
  getBootstrapState,
  listenChatComplete,
  listenChatDelta,
  listenChatError,
  openSettingsWindow,
  submitUserInput,
} from "../../lib/tauri";
import { ExplorationPanel } from "../exploration/ExplorationPanel";

const FIRST_RUN_GREETING =
  "你好！我是喜欢出去玩耍的快乐 AIbb。先配置一个大模型 API，我们再聊天吧。";

interface ChatMessageView {
  id: string;
  role: "user" | "assistant";
  content: string;
}

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
    return {
      code: error.code,
      message:
        "message" in error && typeof error.message === "string"
          ? error.message
          : "操作失败。",
    };
  }
  return { code: "request_failed", message: "操作失败。" };
}

function ChatHeader() {
  return (
    <header className="chat-header">
      <span className="chat-avatar">
        <AibbAvatar />
      </span>
      <div className="chat-identity">
        <h1>AIbb</h1>
        <p><span className="online-dot" aria-hidden="true" />准备出去玩</p>
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

export function ChatPanel() {
  const [bootstrap, setBootstrap] = useState<BootstrapState | null>(null);
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<ChatMessageView[]>([]);
  const [streamingReply, setStreamingReply] = useState("");
  const [activeRequestId, setActiveRequestId] = useState<string | null>(null);
  const [explorationTaskId, setExplorationTaskId] = useState<string | null>(null);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const activeRequest = useRef<string | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const install = async () => {
      const installed = await Promise.all([
        listenChatDelta((event) => {
          if (event.requestId === activeRequest.current) {
            setStreamingReply((reply) => reply + event.delta);
          }
        }),
        listenChatComplete((event) => {
          if (event.requestId !== activeRequest.current) return;
          setMessages((current) => [
            ...current,
            { id: `assistant-${event.requestId}`, role: "assistant", content: event.message },
          ]);
          activeRequest.current = null;
          setActiveRequestId(null);
          setStreamingReply("");
        }),
        listenChatError((event) => {
          if (event.requestId !== activeRequest.current) return;
          setError({ code: event.code, message: event.message });
          activeRequest.current = null;
          setActiveRequestId(null);
          setStreamingReply("");
        }),
      ]);
      if (disposed) installed.forEach((unlisten) => unlisten());
      else unlisteners.push(...installed);
    };
    void install();
    void getBootstrapState()
      .then((state) => {
        if (!disposed) setBootstrap(state);
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(publicError(reason));
      });
    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  async function submit(event: FormEvent) {
    event.preventDefault();
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
      { id: `user-${id}`, role: "user", content: message },
    ]);
    try {
      const disposition = await submitUserInput(message, id);
      if (disposition.kind === "explorationStarted") {
        activeRequest.current = null;
        setActiveRequestId(null);
        setExplorationTaskId(disposition.taskId);
      }
    } catch (reason) {
      activeRequest.current = null;
      setActiveRequestId(null);
      setError(publicError(reason));
    }
  }

  if (!bootstrap && !error) {
    return <main className="panel chat-panel">正在唤醒 AIbb…</main>;
  }
  if (bootstrap && !bootstrap.apiConfigured) {
    return (
      <main className="panel chat-panel" aria-label="AIbb 聊天">
        <ChatHeader />
        <section className="first-run-card">
          <span className="first-run-sparkle" aria-hidden="true">✦</span>
          <h2>你好呀！</h2>
          <p>{FIRST_RUN_GREETING}</p>
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
      </main>
    );
  }

  return (
    <main className="panel chat-panel" aria-label="AIbb 聊天">
      <ChatHeader />
      <section className="conversation" aria-live="polite">
        {messages.length === 0 && !activeRequestId && !explorationTaskId && (
          <div className="chat-welcome">
            <span aria-hidden="true">✦</span>
            <h2>今天想去哪里玩？</h2>
            <p>告诉我一个方向，或者让我自己决定。</p>
          </div>
        )}
        {messages.map((message) => (
          <article key={message.id} className={`message-row ${message.role}`}>
            <span className="message-author">{message.role === "assistant" ? "AIbb" : "你"}</span>
            <p className="message">{message.content}</p>
          </article>
        ))}
        {activeRequestId && (
          <article className="message-row assistant">
            <span className="message-author">AIbb</span>
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
      {explorationTaskId && <ExplorationPanel taskId={explorationTaskId} />}
      <form aria-label="发送消息" className="composer" onSubmit={submit}>
        <label>
          <span className="sr-only">消息</span>
          <textarea
            aria-label="消息"
            placeholder="和 AIbb 说点什么…"
            rows={1}
            value={input}
            onChange={(event) => setInput(event.target.value)}
          />
        </label>
        <button className="send-button" type="submit" disabled={Boolean(activeRequestId) || !input.trim()}>
          发送
        </button>
      </form>
    </main>
  );
}
