import { useEffect, useState, type FormEvent } from "react";
import type { ApiSettings, AppErrorPayload, SaveSettings, WebMode } from "../../contracts";
import { clearMemory, loadSettings, saveSettings, testConnection } from "../../lib/tauri";

const EMPTY_SETTINGS: ApiSettings = {
  apiBase: "",
  model: "",
  webMode: "auto",
  alwaysOnTop: true,
  autostart: false,
  apiConfigured: false,
};

function publicError(reason: unknown): AppErrorPayload {
  const error = reason as Partial<AppErrorPayload>;
  return {
    code: error.code ?? "settings_failed",
    message: error.message ?? "设置操作失败。",
  };
}

export function SettingsPanel() {
  const [settings, setSettings] = useState<ApiSettings>(EMPTY_SETTINGS);
  const [apiKey, setApiKey] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmingClear, setConfirmingClear] = useState(false);

  useEffect(() => {
    let disposed = false;
    void loadSettings()
      .then((loadedSettings) => {
        if (!disposed) {
          setSettings(loadedSettings);
          setApiKey("");
          setLoaded(true);
        }
      })
      .catch((reason: unknown) => {
        if (!disposed) {
          setError(publicError(reason));
          setLoaded(true);
        }
      });
    return () => {
      disposed = true;
    };
  }, []);

  async function save(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setNotice(null);
    const replacementKey = apiKey.trim();
    const payload: SaveSettings = {
      apiBase: settings.apiBase,
      apiKey: replacementKey ? replacementKey : null,
      model: settings.model,
      webMode: settings.webMode,
      alwaysOnTop: settings.alwaysOnTop,
      autostart: settings.autostart,
    };
    try {
      await saveSettings(payload);
      setApiKey("");
      setNotice("设置已保存");
    } catch (reason) {
      setError(publicError(reason));
    }
  }

  async function test() {
    setError(null);
    setNotice(null);
    try {
      await testConnection();
      setNotice("连接成功");
    } catch (reason) {
      setError(publicError(reason));
    }
  }

  async function confirmClear() {
    setError(null);
    try {
      await clearMemory();
      setConfirmingClear(false);
      setNotice("记忆已清除");
    } catch (reason) {
      setError(publicError(reason));
    }
  }

  if (!loaded) return <main className="panel settings-panel">正在加载设置…</main>;

  return (
    <main className="panel settings-panel" aria-label="AIbb 设置">
      <form onSubmit={save}>
        <label>API 地址<input value={settings.apiBase} onChange={(event) => setSettings({ ...settings, apiBase: event.target.value })} /></label>
        <label>API Key<input type="password" autoComplete="new-password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} /></label>
        <label>模型名称<input value={settings.model} onChange={(event) => setSettings({ ...settings, model: event.target.value })} /></label>
        <label>
          联网模式
          <select value={settings.webMode} onChange={(event) => setSettings({ ...settings, webMode: event.target.value as WebMode })}>
            <option value="auto">自动探测</option>
            <option value="force">强制原生联网</option>
            <option value="off">关闭原生联网</option>
          </select>
        </label>
        <label><input type="checkbox" checked={settings.alwaysOnTop} onChange={(event) => setSettings({ ...settings, alwaysOnTop: event.target.checked })} />始终置顶</label>
        <label><input type="checkbox" checked={settings.autostart} onChange={(event) => setSettings({ ...settings, autostart: event.target.checked })} />开机启动</label>
        <div className="button-row">
          <button type="submit">保存设置</button>
          <button type="button" onClick={() => void test()}>测试连接</button>
          <button type="button" onClick={() => setConfirmingClear(true)}>清除记忆</button>
        </div>
      </form>
      {error && <p role="alert">{error.code}：{error.message}</p>}
      {notice && <p role="status">{notice}</p>}
      {confirmingClear && (
        <section role="dialog" aria-label="确认清除记忆" aria-modal="true">
          <p>这会清除本地对话与探索记忆，保留 API 设置。确定继续吗？</p>
          <button type="button" onClick={() => void confirmClear()}>确认清除</button>
          <button type="button" onClick={() => setConfirmingClear(false)}>取消</button>
        </section>
      )}
    </main>
  );
}
