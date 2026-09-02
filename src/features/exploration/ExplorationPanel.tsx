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
    invalid_response: "模型返回的内容无法识别，请稍后重试。",
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
          <p data-testid="exploration-progress">{explorationStatusLabel(status)}</p>
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
