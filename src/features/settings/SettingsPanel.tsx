import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from "react";
import { AibbAvatar } from "../../components/AibbAvatar";
import type {
  AibbProfile,
  ApiSettings,
  AppErrorPayload,
  ArchiveSettings,
  SaveArchiveSettings,
  SaveSettings,
  StructureTemplate,
  WebMode,
} from "../../contracts";
import {
  clearMemory,
  discoverArchiveStructure,
  exitApp,
  listAvailableModels,
  loadAibbProfile,
  loadArchiveSettings,
  loadSettings,
  listenProfileUpdated,
  resetAibbAvatar,
  saveAibbAvatar,
  saveAibbName,
  saveArchiveSettings,
  saveSettings,
  testConnection,
} from "../../lib/tauri";
import { normalizeAvatarFile, type NormalizedAvatarImage } from "../profile/avatarImage";

const EMPTY_SETTINGS: ApiSettings = {
  apiBase: "",
  model: "",
  webMode: "auto",
  alwaysOnTop: true,
  autostart: false,
  apiConfigured: false,
};

const EMPTY_PROFILE: AibbProfile = {
  name: "AIbb",
  avatarDataUrl: null,
  version: 0,
};

const DEEPSEEK_MODEL = "deepseek-v4-flash";

function withProviderDefaults(settings: ApiSettings): ApiSettings {
  const base = settings.apiBase.trim().toLowerCase().replace(/\/+$/, "");
  const isDeepSeek =
    base === "https://api.deepseek.com" ||
    base === "https://api.deepseek.com/v1";
  if (isDeepSeek && !settings.model.trim()) {
    return { ...settings, model: DEEPSEEK_MODEL };
  }
  return settings;
}

function publicError(reason: unknown): AppErrorPayload {
  const error = reason as Partial<AppErrorPayload>;
  const code = error.code ?? "settings_failed";
  const messages: Record<string, string> = {
    authentication_failed:
      "认证失败。请确认 API Key 属于当前 API 地址，重新输入后再点“保存并测试”。",
    model_not_found:
      "找不到这个模型。请点模型名称旁的「检查可用模型」查看该 API 支持的名字，或确认 API 地址正确。",
    modelsUnsupported: "这个 API 不支持查询模型列表，请按官方文档填写模型名称。",
    rate_limited: "请求过于频繁，请稍后再试。",
    request_timeout: "连接超时，请检查网络或 API 地址后重试。",
    provider_unavailable: "模型服务暂时不可用，请稍后再试。",
    invalidSettings: "API 地址必须使用 HTTPS，且模型名称不能为空。",
    invalid_request: "请求被模型服务拒绝，请检查 API 地址和模型名称。",
    credentialStoreUnavailable: "无法读取或保存 API Key，请检查系统凭据服务。",
    settingsRollbackFailed: "设置保存失败，并且无法恢复之前的设置。",
    invalidProfile: "AIbb 资料无效，请检查昵称或头像后重试。",
    profileStorageUnavailable: "无法访问 AIbb 头像，请稍后再试。",
    profileRecoveryRequired: "头像保存未完成，请重新打开设置后再试。",
  };
  return {
    code,
    message: messages[code] ?? "设置操作失败，请检查填写内容后重试。",
  };
}

export function SettingsPanel() {
  const [settings, setSettings] = useState<ApiSettings>(EMPTY_SETTINGS);
  const [profile, setProfile] = useState<AibbProfile>(EMPTY_PROFILE);
  const profileRef = useRef(EMPTY_PROFILE);
  const [profileName, setProfileName] = useState(EMPTY_PROFILE.name);
  const [pendingAvatar, setPendingAvatar] = useState<NormalizedAvatarImage | null>(null);
  const [pendingAvatarDataUrl, setPendingAvatarDataUrl] = useState<string | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<AppErrorPayload | null>(null);
  const [profileError, setProfileError] = useState<AppErrorPayload | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [settingsInFlight, setSettingsInFlight] = useState(false);
  const [profileInFlight, setProfileInFlight] = useState(false);
  const [archive, setArchive] = useState<ArchiveSettings | null>(null);
  const [archiveError, setArchiveError] = useState<AppErrorPayload | null>(null);
  const [archiveNotice, setArchiveNotice] = useState<string | null>(null);
  const [archiveInFlight, setArchiveInFlight] = useState(false);
  const [availableModels, setAvailableModels] = useState<string[] | null>(null);
  const [checkingModels, setCheckingModels] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlistenProfileUpdated: (() => void) | undefined;
    void loadSettings()
      .then((loadedSettings) => {
        if (!disposed) {
          setSettings(withProviderDefaults(loadedSettings));
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
    void loadAibbProfile()
      .then((loadedProfile) => {
        if (!disposed && loadedProfile.version >= profileRef.current.version) {
          applyCurrentProfile(loadedProfile);
        }
      })
      .catch((reason: unknown) => {
        if (!disposed) setProfileError(publicError(reason));
      });
    void loadArchiveSettings()
      .then((loaded) => {
        if (!disposed) setArchive(loaded);
      })
      .catch((reason: unknown) => {
        if (!disposed) setArchiveError(publicError(reason));
      });
    void listenProfileUpdated((updatedProfile) => {
      if (updatedProfile.version > profileRef.current.version) {
        applyCurrentProfile(updatedProfile);
      }
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenProfileUpdated = unlisten;
    }).catch((reason: unknown) => {
      if (!disposed) setProfileError(publicError(reason));
    });
    return () => {
      disposed = true;
      unlistenProfileUpdated?.();
    };
  }, []);

  function applyCurrentProfile(nextProfile: AibbProfile) {
    profileRef.current = nextProfile;
    setProfile(nextProfile);
    setProfileName(nextProfile.name);
    clearPendingAvatar();
  }

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

  async function selectAvatar(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file || profileInFlight) return;

    setProfileError(null);
    setNotice(null);
    setProfileInFlight(true);
    try {
      const normalized = await normalizeAvatarFile(file);
      setPendingAvatar(normalized);
      setPendingAvatarDataUrl(imageDataUrl(normalized.bytes, normalized.mimeType));
      setNotice("新头像已准备好，保存 AIbb 资料后生效。");
    } catch {
      setProfileError({
        code: "invalidProfile",
        message: "头像处理失败，请选择 PNG、JPEG 或 WebP 格式且不超过 5 MiB 的图片。",
      });
    } finally {
      setProfileInFlight(false);
    }
  }

  async function saveProfile() {
    if (profileInFlight) return;
    setProfileError(null);
    setNotice(null);
    setProfileInFlight(true);
    try {
      if (pendingAvatar) {
        let avatarSaved: AibbProfile;
        try {
          avatarSaved = await saveAibbAvatar(pendingAvatar.bytes, pendingAvatar.mimeType);
        } catch (reason) {
          restoreProfileDraft();
          setProfileError(publicError(reason));
          return;
        }
        try {
          const savedProfile = await saveAibbName(profileName.trim());
          applyCurrentProfile(savedProfile);
          setNotice("AIbb 资料已保存");
        } catch {
          applyCurrentProfile(avatarSaved);
          setProfileError({
            code: "profilePartiallySaved",
            message: "头像已保存，昵称未保存。请重新输入昵称后重试。",
          });
        }
        return;
      }
      const savedProfile = await saveAibbName(profileName.trim());
      applyCurrentProfile(savedProfile);
      setNotice("AIbb 资料已保存");
    } catch (reason) {
      restoreProfileDraft();
      setProfileError(publicError(reason));
    } finally {
      setProfileInFlight(false);
    }
  }

  async function resetAvatar() {
    if (profileInFlight) return;
    setProfileError(null);
    setNotice(null);
    setProfileInFlight(true);
    try {
      const savedProfile = await resetAibbAvatar();
      applyCurrentProfile(savedProfile);
      setNotice("已恢复 AIbb 默认头像");
    } catch (reason) {
      setProfileError(publicError(reason));
    } finally {
      setProfileInFlight(false);
    }
  }

  function clearPendingAvatar() {
    setPendingAvatar(null);
    setPendingAvatarDataUrl(null);
  }

  function restoreProfileDraft() {
    setProfileName(profile.name);
    clearPendingAvatar();
  }

  function updateProfileName(value: string) {
    if (Array.from(value).length <= 24) setProfileName(value);
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

  function updateArchiveRoot(value: string) {
    setArchive((current) => (current ? { ...current, root: value } : current));
  }

  function updateArchiveAutoDiscover(checked: boolean) {
    setArchive((current) =>
      current ? { ...current, autoDiscover: checked } : current,
    );
  }

  function updateTemplateName(name: string) {
    setArchive((current) => (current ? { ...current, templateName: name } : current));
  }

  function updateTemplate(
    templateIndex: number,
    patch: Partial<StructureTemplate>,
  ) {
    setArchive((current) =>
      current
        ? {
            ...current,
            templates: current.templates.map((template, index) =>
              index === templateIndex ? { ...template, ...patch } : template,
            ),
          }
        : current,
    );
  }

  function updateCategory(
    templateIndex: number,
    categoryIndex: number,
    patch: Partial<{ name: string; keywords: string[] }>,
  ) {
    setArchive((current) =>
      current
        ? {
            ...current,
            templates: current.templates.map((template, tIndex) =>
              tIndex === templateIndex
                ? {
                    ...template,
                    categories: template.categories.map((category, cIndex) =>
                      cIndex === categoryIndex
                        ? { ...category, ...patch }
                        : category,
                    ),
                  }
                : template,
            ),
          }
        : current,
    );
  }

  function addCategory(templateIndex: number) {
    setArchive((current) =>
      current
        ? {
            ...current,
            templates: current.templates.map((template, index) =>
              index === templateIndex
                ? {
                    ...template,
                    categories: [
                      ...template.categories,
                      { name: "新分类", keywords: [] },
                    ],
                  }
                : template,
            ),
          }
        : current,
    );
  }

  function removeCategory(templateIndex: number, categoryIndex: number) {
    setArchive((current) =>
      current
        ? {
            ...current,
            templates: current.templates.map((template, index) =>
              index === templateIndex
                ? {
                    ...template,
                    categories: template.categories.filter(
                      (_, cIndex) => cIndex !== categoryIndex,
                    ),
                  }
                : template,
            ),
          }
        : current,
    );
  }

  async function saveArchiveSettingsSection() {
    if (!archive || archiveInFlight) return;
    setArchiveError(null);
    setArchiveNotice(null);
    setArchiveInFlight(true);
    try {
      const payload: SaveArchiveSettings = {
        root: archive.root.trim(),
        autoDiscover: archive.autoDiscover,
        templateName: archive.templateName,
        templates: archive.templates,
      };
      await saveArchiveSettings(payload);
      setArchiveNotice("归档设置已保存");
    } catch (reason) {
      setArchiveError(publicError(reason));
    } finally {
      setArchiveInFlight(false);
    }
  }

  async function scanArchiveStructure() {
    if (archiveInFlight) return;
    setArchiveError(null);
    setArchiveNotice(null);
    setArchiveInFlight(true);
    try {
      const discovered = await discoverArchiveStructure();
      if (!discovered) {
        setArchiveNotice("归档区还没有可识别的结构，先归档一个文件试试。");
      } else {
        const hierarchy = discovered.hierarchy.join(" → ");
        const categories = discovered.categories.map((category) => category.name).join("、");
        setArchiveNotice(`扫描到「${hierarchy}」结构，分类：${categories}`);
      }
    } catch (reason) {
      setArchiveError(publicError(reason));
    } finally {
      setArchiveInFlight(false);
    }
  }

  async function checkAvailableModels() {
    if (checkingModels) return;
    setError(null);
    setNotice(null);
    setCheckingModels(true);
    try {
      const models = await listAvailableModels();
      setAvailableModels(models);
      if (models.length === 0) {
        setNotice("该 API 返回的模型列表为空，请检查 API 地址。");
      }
    } catch (reason) {
      setAvailableModels([]);
      setError(publicError(reason));
    } finally {
      setCheckingModels(false);
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

      <form className="settings-form" aria-busy={settingsInFlight || profileInFlight} onSubmit={save}>
        <section className="profile-card" aria-labelledby="profile-heading">
          <div className="profile-preview" aria-label="AIbb 资料预览">
            <span className="profile-avatar"><AibbAvatar avatarDataUrl={pendingAvatarDataUrl ?? profile.avatarDataUrl} name={profileName.trim() || profile.name} /></span>
            <div>
              <h2 id="profile-heading">AIbb 资料</h2>
              <strong>{profileName.trim() || profile.name}</strong>
              <p>昵称和头像只保存在这台设备上。</p>
            </div>
          </div>
          <label className="field">
            <span>AIbb 昵称</span>
            <input
              disabled={profileInFlight}
              value={profileName}
              onChange={(event) => updateProfileName(event.target.value)}
            />
          </label>
          <div className="profile-actions">
            <label className="button secondary file-button">
              <span>选择头像</span>
              <input aria-label="选择头像" accept="image/png,image/jpeg,image/webp" disabled={profileInFlight} type="file" onChange={(event) => void selectAvatar(event)} />
            </label>
            <button className="button secondary" disabled={profileInFlight || !(pendingAvatarDataUrl ?? profile.avatarDataUrl)} type="button" onClick={() => void resetAvatar()}>恢复默认头像</button>
            <button className="button primary" disabled={profileInFlight} type="button" onClick={() => void saveProfile()}>保存 AIbb 资料</button>
          </div>
        </section>
        {profileError && <p className="feedback error" role="alert">{profileError.message}</p>}

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
              required
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

        <div className="model-actions">
          <button
            aria-label="检查可用模型"
            className="text-button model-check-button"
            disabled={checkingModels || !settings.apiBase.trim()}
            type="button"
            onClick={() => void checkAvailableModels()}
          >
            {checkingModels ? "正在查询…" : "检查可用模型"}
          </button>
          <small className="model-hint">
            DeepSeek 官方模型：deepseek-v4-flash、deepseek-v4-pro
          </small>
        </div>

        {availableModels !== null && availableModels.length > 0 && (
          <div className="model-list" aria-label="可用模型">
            <span className="model-list-label">该 API 可用的模型：</span>
            <div className="model-chips">
              {availableModels.map((model) => {
                const current = settings.model.trim() === model;
                return (
                  <button
                    key={model}
                    aria-pressed={current}
                    className={`model-chip${current ? " selected" : ""}`}
                    type="button"
                    onClick={() => setSettings({ ...settings, model })}
                  >
                    {model}
                  </button>
                );
              })}
            </div>
          </div>
        )}
        {availableModels !== null &&
          availableModels.length > 0 &&
          settings.model.trim() &&
          !availableModels.includes(settings.model.trim()) && (
            <p className="feedback warning" role="status">
              当前模型「{settings.model.trim()}」不在该 API 的可用列表里，点上方名称即可填入。
            </p>
          )}

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

        {archive && (
          <section className="archive-settings" aria-labelledby="archive-heading">
            <h2 id="archive-heading">文件归档</h2>
            <p className="settings-hint">
              把文件拖到 AIbb 身上，AIbb 会自动分类归档到项目目录，原文件保留不动。
            </p>
            <label className="field">
              <span>归档根目录</span>
              <input
                disabled={archiveInFlight}
                value={archive.root}
                onChange={(event) => updateArchiveRoot(event.target.value)}
                placeholder="留空使用默认目录「本地归档」"
              />
            </label>
            <div className="toggle-row">
              <label className="toggle-option">
                <input
                  type="checkbox"
                  disabled={archiveInFlight}
                  checked={archive.autoDiscover}
                  onChange={(event) => updateArchiveAutoDiscover(event.target.checked)}
                />
                <span>自动识别归档区已有结构</span>
              </label>
            </div>
            <label className="field">
              <span>结构库模板</span>
              <select
                disabled={archiveInFlight}
                value={archive.templateName}
                onChange={(event) => updateTemplateName(event.target.value)}
              >
                {archive.templates.map((template) => (
                  <option key={template.name} value={template.name}>
                    {template.name}
                  </option>
                ))}
              </select>
            </label>
            {archive.templates.map((template, templateIndex) => (
              <fieldset key={template.name} className="template-editor">
                <legend>模板「{template.name}」</legend>
                <label className="field">
                  <span>层级</span>
                  <input
                    disabled={archiveInFlight}
                    value={template.hierarchy.join(", ")}
                    onChange={(event) =>
                      updateTemplate(templateIndex, {
                        hierarchy: splitList(event.target.value),
                      })
                    }
                  />
                  <small>支持 week（周）、category（分类），例如：week, category</small>
                </label>
                <div className="toggle-row">
                  <label className="toggle-option">
                    <input
                      type="checkbox"
                      disabled={archiveInFlight}
                      checked={template.includeSource}
                      onChange={(event) =>
                        updateTemplate(templateIndex, { includeSource: event.target.checked })
                      }
                    />
                    <span>保留原始文件到「源文件」备份</span>
                  </label>
                </div>
                <div className="category-editor" aria-label="分类规则">
                  {template.categories.map((category, categoryIndex) => (
                    <div
                      key={`${templateIndex}-${categoryIndex}`}
                      className="category-row"
                    >
                      <input
                        aria-label="分类名"
                        disabled={archiveInFlight}
                        value={category.name}
                        onChange={(event) =>
                          updateCategory(templateIndex, categoryIndex, {
                            name: event.target.value,
                          })
                        }
                      />
                      <input
                        aria-label="关键词"
                        disabled={archiveInFlight}
                        placeholder="关键词，逗号分隔"
                        value={category.keywords.join(", ")}
                        onChange={(event) =>
                          updateCategory(templateIndex, categoryIndex, {
                            keywords: splitList(event.target.value),
                          })
                        }
                      />
                      <button
                        aria-label={`删除分类 ${category.name}`}
                        className="category-remove"
                        disabled={archiveInFlight}
                        type="button"
                        onClick={() => removeCategory(templateIndex, categoryIndex)}
                      >
                        ✕
                      </button>
                    </div>
                  ))}
                </div>
                <button
                  className="text-button"
                  disabled={archiveInFlight}
                  type="button"
                  onClick={() => addCategory(templateIndex)}
                >
                  + 添加分类
                </button>
              </fieldset>
            ))}
            <div className="button-row">
              <button
                className="button secondary"
                disabled={archiveInFlight}
                type="button"
                onClick={() => void scanArchiveStructure()}
              >
                扫描归档区
              </button>
              <button
                className="button primary"
                disabled={archiveInFlight}
                type="button"
                onClick={() => void saveArchiveSettingsSection()}
              >
                保存归档设置
              </button>
            </div>
            {archiveError && <p className="feedback error" role="alert">{archiveError.message}</p>}
            {archiveNotice && <p className="feedback success" role="status">{archiveNotice}</p>}
          </section>
        )}

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
        <footer className="settings-footer">
          AIbb · 本地优先 · 对话、记忆与归档记录只保存在这台电脑上
        </footer>
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

function imageDataUrl(bytes: Uint8Array, mimeType: string): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:${mimeType};base64,${btoa(binary)}`;
}

function splitList(value: string): string[] {
  return value
    .split(/[,，、\s]+/)
    .map((part) => part.trim())
    .filter(Boolean);
}
