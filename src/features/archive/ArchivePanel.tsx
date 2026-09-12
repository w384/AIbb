import { useEffect, useState } from "react";
import type {
  AppErrorPayload,
  ArchiveFileResult,
  ArchiveLedgerEntry,
} from "../../contracts";
import {
  archiveFiles,
  archiveLedger,
  listenArchivePending,
  takePendingArchivePaths,
} from "../../lib/tauri";

const EMPTY_LEDGER: ArchiveLedgerEntry[] = [];

function publicArchiveError(reason: unknown): AppErrorPayload {
  const error = reason as Partial<AppErrorPayload>;
  const code = error.code ?? "archiveFailed";
  const messages: Record<string, string> = {
    invalidArchiveSettings: "归档设置无效，请先到设置里检查归档根目录。",
    archiveFailed: "归档任务执行失败，请稍后重试。",
    storageUnavailable: "本地数据暂时无法访问，请重启 AIbb 后重试。",
  };
  return {
    code,
    message: messages[code] ?? "归档失败，请检查文件后重试。",
  };
}

function baseName(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  return normalized.slice(normalized.lastIndexOf("/") + 1) || path;
}

function ResultLine({ result }: { result: ArchiveFileResult }) {
  if (result.ok) {
    return (
      <li className="archive-result ok">
        <span className="archive-result-mark" aria-hidden="true">✓</span>
        <span className="archive-result-text">
          <strong>{result.fileName}</strong>
          <span>
            已归档到 <code>{result.archiveRel}</code>
            {result.category && <>（分类 {result.category}，版本 {result.version}）</>}
          </span>
        </span>
      </li>
    );
  }
  if (result.duplicate) {
    return (
      <li className="archive-result duplicate">
        <span className="archive-result-mark" aria-hidden="true">↻</span>
        <span className="archive-result-text">
          <strong>{result.fileName}</strong>
          <span>{result.reason}</span>
        </span>
      </li>
    );
  }
  return (
    <li className="archive-result failed">
      <span className="archive-result-mark" aria-hidden="true">✕</span>
      <span className="archive-result-text">
        <strong>{result.fileName}</strong>
        <span>{result.reason}</span>
      </span>
    </li>
  );
}

function formatLedgerTime(millis: number): string {
  return new Date(millis).toLocaleString("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function ArchivePanel() {
  const [pending, setPending] = useState<string[]>([]);
  const [project, setProject] = useState("");
  const [working, setWorking] = useState(false);
  const [results, setResults] = useState<ArchiveFileResult[] | null>(null);
  const [ledger, setLedger] = useState<ArchiveLedgerEntry[]>(EMPTY_LEDGER);
  const [error, setError] = useState<AppErrorPayload | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlistenPending: (() => void) | undefined;
    const take = () => {
      void takePendingArchivePaths()
        .then((paths) => {
          if (disposed || paths.length === 0) return;
          setResults(null);
          setProject("");
          setError(null);
          setPending(paths);
        })
        .catch(() => {});
    };
    void archiveLedger(8)
      .then((entries) => {
        if (!disposed) setLedger(entries);
      })
      .catch(() => {});
    void listenArchivePending(() => {
      take();
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenPending = unlisten;
      })
      .catch(() => {});
    // Fresh window: consume whatever the pet window stashed before opening.
    take();
    return () => {
      disposed = true;
      unlistenPending?.();
    };
  }, []);

  async function startArchive() {
    const name = project.trim();
    if (!name || working) return;
    if (name.includes("/") || name.includes("\\") || name === "." || name === "..") {
      setError({
        code: "invalidProject",
        message: "项目名不能包含路径分隔符。",
      });
      return;
    }
    setError(null);
    setWorking(true);
    try {
      const outcomes = await archiveFiles(pending, name);
      setResults(outcomes);
      setPending([]);
      setLedger(await archiveLedger(8));
    } catch (reason) {
      setError(publicArchiveError(reason));
    } finally {
      setWorking(false);
    }
  }

  function dismiss() {
    setPending([]);
    setResults(null);
    setProject("");
    setError(null);
  }

  const visible = pending.length > 0 || results !== null;
  if (!visible) return null;

  const fileNameCount = pending.length;

  return (
    <section className="archive-panel" aria-label="文件归档">
      <header className="archive-panel-header">
        <span className="archive-panel-mark" aria-hidden="true">▣</span>
        <div>
          <h2>文件归档</h2>
          <p>
            {results === null
              ? `${fileNameCount} 个文件等待归档`
              : `${results.length} 个文件已处理`}
          </p>
        </div>
        <button
          aria-label="收起归档面板"
          className="archive-dismiss"
          type="button"
          onClick={dismiss}
        >
          ✕
        </button>
      </header>

      {results === null ? (
        <form
          aria-label="归档到项目"
          className="archive-form"
          onSubmit={(event) => {
            event.preventDefault();
            void startArchive();
          }}
        >
          <ul className="archive-file-list" aria-label="等待归档的文件">
            {pending.map((path) => (
              <li key={path}>
                <span aria-hidden="true">📄</span>
                {baseName(path)}
              </li>
            ))}
          </ul>
          <label className="archive-project-field">
            <span>归档到项目</span>
            <input
              autoFocus
              placeholder="例如：0828"
              value={project}
              onChange={(event) => setProject(event.target.value)}
            />
            <small>文件会归档到「归档区/项目名/…」，原文件保留不动。</small>
          </label>
          <button
            className="archive-submit"
            type="submit"
            disabled={working || !project.trim()}
          >
            {working ? "正在归档…" : "开始归档"}
          </button>
        </form>
      ) : (
        <ul className="archive-results" aria-label="归档结果">
          {results.map((result) => (
            <ResultLine key={result.fileName} result={result} />
          ))}
        </ul>
      )}

      {error && (
        <p className="archive-feedback" role="alert">
          <span className="archive-error-code">{error.code}</span>
          <span>{error.message}</span>
        </p>
      )}

      {ledger.length > 0 && (
        <div className="archive-ledger">
          <h3>最近归档</h3>
          <ul>
            {ledger.map((entry) => (
              <li key={entry.id}>
                <span className="archive-ledger-file">{entry.fileName}</span>
                <span className="archive-ledger-meta">
                  {entry.project} · {entry.category} · {entry.version}
                </span>
                <span className="archive-ledger-time">
                  {formatLedgerTime(entry.createdAt)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
