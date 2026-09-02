import { useEffect, useState } from "react";
import type {
  AppErrorPayload,
  ExplorationResult,
  ExplorationStatus,
} from "../../contracts";
import {
  cancelExploration,
  listenExplorationComplete,
  listenExplorationError,
  listenExplorationProgress,
  startExploration,
} from "../../lib/tauri";

interface ExplorationPanelProps {
  taskId: string;
}

function publicExplorationError(error: AppErrorPayload): string {
  const messages: Record<string, string> = {
    invalidSettings: "请打开设置，填写并保存 API 地址和模型名称后重试。",
    invalid_request: "请求被模型服务拒绝，请打开设置确认模型名称已经保存。",
    model_not_found: "找不到当前模型，请打开设置检查模型名称。",
    authentication_failed: "API Key 认证失败，请打开设置重新检查。",
    rate_limited: "请求过于频繁，请稍后再让 AIbb 出去玩。",
    request_timeout: "探索请求超时，请稍后重试。",
    provider_unavailable: "模型服务暂时不可用，请稍后重试。",
  };
  return messages[error.code] ?? error.message;
}

export function ExplorationPanel({ taskId }: ExplorationPanelProps) {
  const [currentTaskId, setCurrentTaskId] = useState(taskId);
  const [status, setStatus] = useState<ExplorationStatus>("queued");
  const [result, setResult] = useState<ExplorationResult | null>(null);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const [allowing, setAllowing] = useState(false);

  useEffect(() => {
    setCurrentTaskId(taskId);
    setStatus("queued");
    setResult(null);
    setError(null);
  }, [taskId]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    void Promise.all([
      listenExplorationProgress((event) => {
        if (event.taskId === currentTaskId) setStatus(event.status);
      }),
      listenExplorationComplete((event) => {
        if (event.taskId !== currentTaskId) return;
        setStatus("completed");
        setResult(event.result);
      }),
      listenExplorationError((event) => {
        if (event.taskId !== currentTaskId) return;
        setStatus(event.code === "cancelled" ? "cancelled" : "failed");
        setError({ code: event.code, message: event.message });
      }),
    ]).then((installed) => {
      if (disposed) installed.forEach((unlisten) => unlisten());
      else unlisteners.push(...installed);
    });
    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [currentTaskId]);

  async function allowNextOuting() {
    if (!result) return;
    setAllowing(true);
    setError(null);
    try {
      const nextTaskId = await startExploration(result.nextOutingRequest);
      setCurrentTaskId(nextTaskId);
      setStatus("queued");
      setResult(null);
    } catch (reason) {
      const payload = reason as Partial<AppErrorPayload>;
      setError({
        code: payload.code ?? "exploration_start_failed",
        message: payload.message ?? "无法开始探索。",
      });
    } finally {
      setAllowing(false);
    }
  }

  return (
    <section className="exploration-panel" aria-label="探索结果">
      <header className="exploration-header">
        <span aria-hidden="true">✦</span>
        <div>
          <h2>AIbb 的出游记录</h2>
          <p data-testid="exploration-progress">{status}</p>
        </div>
      </header>
      {!result && !error && (
        <button className="exploration-secondary-button" type="button" onClick={() => void cancelExploration(currentTaskId)}>
          取消探索
        </button>
      )}
      {error && <p className="chat-feedback" role="alert">{publicExplorationError(error)}</p>}
      {result && (
        <>
          <ol className="exploration-results">
            {result.items.map((item, index) => (
              <li data-testid="exploration-item" key={`${index}-${item}`}>
                {item}
              </li>
            ))}
          </ol>
          <div className="next-outing-card">
            <span>AIbb 还想去</span>
            <p data-testid="next-outing-request">{result.nextOutingRequest}</p>
          </div>
          <button className="chat-primary-button" type="button" disabled={allowing} onClick={() => void allowNextOuting()}>
            允许出去玩
          </button>
        </>
      )}
    </section>
  );
}
