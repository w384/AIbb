import { useEffect, useState, type FormEvent } from "react";
import type { ApiSettings, AppErrorPayload, SaveSettings, WebMode } from "../../contracts";
import {
  clearMemory,
  exitApp,
  loadSettings,
  saveSettings,
  testConnection,
} from "../../lib/tauri";

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
  const code = error.code ?? "settings_failed";
  const messages: Record<string, string> = {
    authentication_failed:
      "认证失败。请确认 API Key 属于当前 API 地址，重新输入后再点“保存并测试”。",
    model_not_found: "找不到这个模型，请检查模型名称后重试。",
    rate_limited: "请求过于频繁，请稍后再试。",
    request_timeout: "连接超时，请检查网络或 API 地址后重试。",
    provider_unavailable: "模型服务暂时不可用，请稍后再试。",
    credentialStoreUnavailable: "无法读取或保存 API Key，请检查系统凭据服务。",
    settingsRollbackFailed: "设置保存失败，并且无法恢复之前的设置。",
  };
  return {
    code,
    message: messages[code] ?? "设置操作失败，请检查填写内容后重试。",
  };
}

export function SettingsPanel() {
  const [settings, setSettings] = useState<ApiSettings>(EMPTY_SETTINGS);
  const [apiKey, setApiKey] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [settingsInFlight, setSettingsInFlight] = useState(false);

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
    if (settingsInFlight) return;
    setError(null);
    setNotice(null);
    const payload = currentPayload();
    const submittedKey = apiKey;
    setSettingsInFlight(true);
    try {
      await saveSettings(payload);
      markSaved(payload, submittedKey);
      setNotice("设置已保存");
    } catch (reason) {
      setError(publicError(reason));
    } finally {
      setSettingsInFlight(false);
    }
  }

  function currentPayload(): SaveSettings {
    const replacementKey = apiKey.trim();
    return {
      apiBase: settings.apiBase.trim(),
      apiKey: replacementKey ? replacementKey : null,
      model: settings.model.trim(),
      webMode: settings.webMode,
      alwaysOnTop: settings.alwaysOnTop,
      autostart: settings.autostart,
    };
  }

  async function saveAndTest() {
    if (settingsInFlight) return;
    setError(null);
    setNotice(null);
    const payload = currentPayload();
    const submittedKey = apiKey;
    setSettingsInFlight(true);
    try {
      await saveSettings(payload);
      await testConnection();
      markSaved(payload, submittedKey);
      setNotice("设置已保存，连接成功");
    } catch (reason) {
      setError(publicError(reason));
    } finally {
      setSettingsInFlight(false);
    }
  }

  function markSaved(payload: SaveSettings, submittedKey: string) {
    setSettings((current) => ({
      ...current,
      apiBase: payload.apiBase,
      model: payload.model,
      apiConfigured: current.apiConfigured || Boolean(payload.apiKey),
    }));
    setApiKey((current) => (current === submittedKey ? "" : current));
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
      <header className="settings-header">
        <span className="settings-mark" aria-hidden="true">◎</span>
        <div>
          <h1>AIbb 设置</h1>
          <p>配置一个大模型，AIbb 就可以出去玩啦。</p>
        </div>
      </header>

      <form className="settings-form" aria-busy={settingsInFlight} onSubmit={save}>
        <label className="field">
          <span>API 地址</span>
          <input
            type="url"
            disabled={settingsInFlight}
            value={settings.apiBase}
            onChange={(event) => setSettings({ ...settings, apiBase: event.target.value })}
            placeholder="https://api.deepseek.com"
          />
        </label>

        <div className="field">
          <label htmlFor="api-key">API Key</label>
          <input
            id="api-key"
            type="password"
            disabled={settingsInFlight}
            autoComplete="new-password"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder={settings.apiConfigured ? "已安全保存" : "请输入 API Key"}
          />
          <small>留空表示继续使用已保存的密钥</small>
        </div>

        <div className="settings-grid">
          <label className="field">
            <span>模型名称</span>
            <input
              disabled={settingsInFlight}
              value={settings.model}
              onChange={(event) => setSettings({ ...settings, model: event.target.value })}
              placeholder="deepseek-v4-flash"
            />
          </label>
          <label className="field">
            <span>联网模式</span>
            <select
              disabled={settingsInFlight}
              value={settings.webMode}
              onChange={(event) => setSettings({ ...settings, webMode: event.target.value as WebMode })}
            >
              <option value="auto">自动探测</option>
              <option value="force">强制原生联网</option>
              <option value="off">关闭原生联网</option>
            </select>
          </label>
        </div>

        <div className="toggle-row">
          <label className="toggle-option">
            <input
              type="checkbox"
              disabled={settingsInFlight}
              checked={settings.alwaysOnTop}
              onChange={(event) => setSettings({ ...settings, alwaysOnTop: event.target.checked })}
            />
            <span>始终置顶</span>
          </label>
          <label className="toggle-option">
            <input
              type="checkbox"
              disabled={settingsInFlight}
              checked={settings.autostart}
              onChange={(event) => setSettings({ ...settings, autostart: event.target.checked })}
            />
            <span>开机启动</span>
          </label>
        </div>

        <div className="button-row">
          <button className="button secondary" type="submit" disabled={settingsInFlight}>仅保存</button>
          <button className="button primary" type="button" disabled={settingsInFlight} onClick={() => void saveAndTest()}>
            保存并测试
          </button>
        </div>

        <div className="secondary-actions">
          <button className="text-button" type="button" onClick={() => setConfirmingClear(true)}>清除记忆</button>
          <button className="text-button danger" type="button" onClick={() => void exitApp()}>退出 AIbb</button>
        </div>
      </form>
      {error && <p className="feedback error" role="alert">{error.message}</p>}
      {notice && <p className="feedback success" role="status">{notice}</p>}
      {confirmingClear && (
        <section className="confirm-dialog" role="dialog" aria-label="确认清除记忆" aria-modal="true">
          <div className="confirm-card">
            <h2>清除 AIbb 的记忆？</h2>
            <p>这会清除本地对话与探索记忆，但保留 API 设置。</p>
            <div className="button-row">
              <button className="button danger-button" type="button" onClick={() => void confirmClear()}>确认清除</button>
              <button className="button secondary" type="button" onClick={() => setConfirmingClear(false)}>取消</button>
            </div>
          </div>
        </section>
      )}
    </main>
  );
}
