import { useEffect, useRef, useState, type FormEvent } from "react";
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
        <p>{FIRST_RUN_GREETING}</p>
        <button type="button" onClick={() => void openSettingsWindow()}>
          打开 API 设置
        </button>
        {error && <p role="alert">{error.code}：{error.message}</p>}
      </main>
    );
  }

  return (
    <main className="panel chat-panel" aria-label="AIbb 聊天">
      <section className="conversation" aria-live="polite">
        {messages.map((message) => (
          <p key={message.id} className={`message ${message.role}`}>
            {message.content}
          </p>
        ))}
        {activeRequestId && (
          <p className="message assistant" data-testid="streaming-reply">
            {streamingReply || "AIbb 正在想…"}
          </p>
        )}
      </section>
      {error && <p role="alert">{error.code}：{error.message}</p>}
      {explorationTaskId && <ExplorationPanel taskId={explorationTaskId} />}
      <form aria-label="发送消息" className="composer" onSubmit={submit}>
        <label>
          <span>消息</span>
          <textarea
            aria-label="消息"
            value={input}
            onChange={(event) => setInput(event.target.value)}
          />
        </label>
        <button type="submit" disabled={Boolean(activeRequestId) || !input.trim()}>
          发送
        </button>
      </form>
    </main>
  );
}
