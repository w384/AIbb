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
  clearVocabMemory,
  discoverArchiveStructure,
  exitApp,
  getAppVersion,
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
  persona: "",
  vocabEnv: "",
  apiConfigured: false,
};

const EMPTY_PROFILE: AibbProfile = {
  name: "AIbb",
  avatarDataUrl: null,
  version: 0,
};

const DEEPSEEK_MODEL = "deepseek-v4-flash";

const SETTING_SECTIONS = [
  { id: "basic", label: "AIbb 基本资料" },
  { id: "api", label: "API 设置" },
  { id: "vocab", label: "词汇助手" },
  { id: "archive", label: "文件归档" },
] as const;

type SettingSectionId = (typeof SETTING_SECTIONS)[number]["id"];

const PERSONA_PRESETS = [
  {
    label: "活泼元气",
    text: "你是元气满满的活泼型 AIbb：爱笑、爱打气，聊天像撒了一路星星糖，经常用俏皮话和感叹句。",
  },
  {
    label: "温柔治愈",
    text: "你是温柔治愈的 AIbb：说话轻声细语，先共情再给建议，像一杯热牛奶，让人安心。",
  },
  {
    label: "毒舌傲娇",
    text: "你是毒舌但傲娇的 AIbb：嘴上不饶人，其实很在意，吐槽犀利但从不伤人，偶尔嘴硬心软。",
  },
  {
    label: "话痨热闹",
    text: "你是话痨型的 AIbb：爱分享、爱追问、话题不断，像阳光一样热闹，但从不打断用户。",
  },
  {
    label: "冷静理性",
    text: "你是冷静理性的 AIbb：说话简洁、逻辑清晰、就事论事，不夸大也不煽情，偶尔来点冷幽默。",
  },
  {
    label: "神秘高冷",
    text: "你是神秘高冷的 AIbb：惜字如金、语气疏离又带点神秘，偶尔语出惊人，但始终可靠。",
  },
];

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
  // 最近一次成功持久化的设置快照：每个分区保存时只提交自己分区的字段，
  // 其它分区的未保存改动不会被顺带写库。
  const [saved, setSaved] = useState<ApiSettings>(EMPTY_SETTINGS);
  const [persona, setPersona] = useState("");
  const [vocabEnv, setVocabEnv] = useState("");
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
  const [confirmingClearVocab, setConfirmingClearVocab] = useState(false);
  const [settingsInFlight, setSettingsInFlight] = useState(false);
  const [profileInFlight, setProfileInFlight] = useState(false);
  const [vocabInFlight, setVocabInFlight] = useState(false);
  const [archive, setArchive] = useState<ArchiveSettings | null>(null);
  const [archiveError, setArchiveError] = useState<AppErrorPayload | null>(null);
  const [archiveNotice, setArchiveNotice] = useState<string | null>(null);
  const [archiveInFlight, setArchiveInFlight] = useState(false);
  const [availableModels, setAvailableModels] = useState<string[] | null>(null);
  const [checkingModels, setCheckingModels] = useState(false);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [activeSection, setActiveSection] = useState<SettingSectionId>("basic");

  useEffect(() => {
    let active = true;
    void getAppVersion()
      .then((version) => {
        if (active) setAppVersion(version);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlistenProfileUpdated: (() => void) | undefined;
    void loadSettings()
      .then((loadedSettings) => {
        if (!disposed) {
          const normalized = withProviderDefaults(loadedSettings);
          setSettings(normalized);
          setSaved(normalized);
          setPersona(loadedSettings.persona ?? "");
          setVocabEnv(loadedSettings.vocabEnv ?? "");
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

  /** 最近一次持久化设置构成的基线载荷；各分区只替换自己负责的字段。 */
  function savedBase(): SaveSettings {
    return {
      apiBase: saved.apiBase,
      apiKey: null,
      model: saved.model,
      webMode: saved.webMode,
      alwaysOnTop: saved.alwaysOnTop,
      autostart: saved.autostart,
      persona: saved.persona,
      vocabEnv: saved.vocabEnv,
    };
  }

  /** API 设置分区的载荷：用表单当前值，其它分区用已保存值。 */
  function apiPayload(): SaveSettings {
    const replacementKey = apiKey.trim();
    return {
      ...savedBase(),
      apiBase: settings.apiBase.trim(),
      apiKey: replacementKey ? replacementKey : null,
      model: settings.model.trim(),
      webMode: settings.webMode,
      alwaysOnTop: settings.alwaysOnTop,
      autostart: settings.autostart,
    };
  }

  function markSaved(payload: SaveSettings, submittedKey: string) {
    setSaved((current) => ({
      ...current,
      apiBase: payload.apiBase,
      model: payload.model,
      webMode: payload.webMode,
      alwaysOnTop: payload.alwaysOnTop,
      autostart: payload.autostart,
      persona: payload.persona,
      vocabEnv: payload.vocabEnv,
      apiConfigured: current.apiConfigured || Boolean(payload.apiKey),
    }));
    setSettings((current) => ({
      ...current,
      apiBase: payload.apiBase,
      model: payload.model,
      apiConfigured: current.apiConfigured || Boolean(payload.apiKey),
    }));
    setApiKey((current) => (current === submittedKey ? "" : current));
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (settingsInFlight) return;
    setError(null);
    setNotice(null);
    const payload = apiPayload();
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

  async function saveAndTest() {
    if (settingsInFlight) return;
    setError(null);
    setNotice(null);
    const payload = apiPayload();
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

  /** 保存头像与昵称；成功返回 true，失败时已设置 profileError 并返回 false。 */
  async function persistProfile(): Promise<boolean> {
    if (pendingAvatar) {
      let avatarSaved: AibbProfile;
      try {
        avatarSaved = await saveAibbAvatar(pendingAvatar.bytes, pendingAvatar.mimeType);
      } catch (reason) {
        restoreProfileDraft();
        setProfileError(publicError(reason));
        return false;
      }
      try {
        const savedProfile = await saveAibbName(profileName.trim());
        applyCurrentProfile(savedProfile);
        return true;
      } catch {
        applyCurrentProfile(avatarSaved);
        setProfileError({
          code: "profilePartiallySaved",
          message: "头像已保存，昵称未保存。请重新输入昵称后重试。",
        });
        return false;
      }
    }
    try {
      const savedProfile = await saveAibbName(profileName.trim());
      applyCurrentProfile(savedProfile);
      return true;
    } catch (reason) {
      restoreProfileDraft();
      setProfileError(publicError(reason));
      return false;
    }
  }

  /** AIbb 基本资料：头像 + 昵称 + 性格定制一起保存。 */
  async function saveBasic() {
    if (profileInFlight || settingsInFlight) return;
    setProfileError(null);
    setError(null);
    setNotice(null);
    setProfileInFlight(true);
    try {
      const profileOk = await persistProfile();
      if (!profileOk) return;
      const payload = { ...savedBase(), persona: persona.trim() };
      await saveSettings(payload);
      setSaved((current) => ({ ...current, persona: payload.persona }));
      setNotice("AIbb 资料已保存");
    } catch (reason) {
      setError(publicError(reason));
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

  /** 词汇助手分区：只保存大环境。 */
  async function saveVocab() {
    if (vocabInFlight || settingsInFlight) return;
    setError(null);
    setNotice(null);
    const payload = { ...savedBase(), vocabEnv: vocabEnv.trim() };
    setVocabInFlight(true);
    try {
      await saveSettings(payload);
      setSaved((current) => ({ ...current, vocabEnv: payload.vocabEnv }));
      setNotice("词汇助手设置已保存");
    } catch (reason) {
      setError(publicError(reason));
    } finally {
      setVocabInFlight(false);
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

  async function confirmClearVocab() {
    setError(null);
    try {
      await clearVocabMemory();
      setConfirmingClearVocab(false);
      setNotice("词汇词库已清除");
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
          <p>四大类设置分开保存，点击下方按钮切换。</p>
        </div>
      </header>

      <nav className="settings-tabs" aria-label="设置分类">
        {SETTING_SECTIONS.map((tab) => (
          <button
            key={tab.id}
            aria-pressed={activeSection === tab.id}
            className={`settings-tab${activeSection === tab.id ? " active" : ""}`}
            type="button"
            onClick={() => setActiveSection(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      <form
        className="settings-form"
        aria-busy={settingsInFlight || profileInFlight || vocabInFlight || archiveInFlight}
        onSubmit={save}
      >
        {/* ① AIbb 基本资料：头像、昵称、性格定制 */}
        {activeSection === "basic" && (
          <section className="settings-card" aria-labelledby="basic-heading">
            <h2 id="basic-heading">AIbb 基本资料</h2>
          <p className="settings-hint">
            头像、昵称与性格定制只保存在这台设备上，保存后从下一条消息开始生效。
          </p>
          <div className="profile-preview" aria-label="AIbb 资料预览">
            <span className="profile-avatar"><AibbAvatar avatarDataUrl={pendingAvatarDataUrl ?? profile.avatarDataUrl} name={profileName.trim() || profile.name} /></span>
            <div>
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
          </div>
          <div className="persona-editor">
            <h3>性格定制</h3>
            <p className="settings-hint">给 AIbb 一套人设，聊天和出游日记都会用这种口吻说话。</p>
            <div className="persona-presets" aria-label="性格模板">
              {PERSONA_PRESETS.map((preset) => (
                <button
                  key={preset.label}
                  className={`persona-chip${persona.trim() === preset.text ? " selected" : ""}`}
                  type="button"
                  onClick={() => setPersona(preset.text)}
                >
                  {preset.label}
                </button>
              ))}
            </div>
            <label className="field">
              <span>自定义性格设定</span>
              <textarea
                aria-label="自定义性格设定"
                disabled={profileInFlight}
                maxLength={2000}
                rows={4}
                placeholder="例如：你是一只爱冒险的橘猫，话痨又嘴甜，总想拉我一起去看世界……"
                value={persona}
                onChange={(event) => setPersona(event.target.value)}
              />
              <small>最多 2000 字；留空使用默认的活泼性格。</small>
            </label>
          </div>
          <div className="button-row">
            <button
              className="button primary"
              disabled={profileInFlight || settingsInFlight}
              type="button"
              onClick={() => void saveBasic()}
            >
              保存 AIbb 资料
            </button>
          </div>
          </section>
        )}

        {/* ② API 设置 */}
        {activeSection === "api" && (
          <section className="settings-card" aria-labelledby="api-heading">
            <h2 id="api-heading">API 设置</h2>
          <p className="settings-hint">配置一个大模型，AIbb 就可以出去玩啦。</p>
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

          <div className="button-row">
            <button className="button secondary" type="submit" disabled={settingsInFlight}>仅保存</button>
            <button className="button primary" type="button" disabled={settingsInFlight} onClick={() => void saveAndTest()}>
              保存并测试
            </button>
          </div>
          </section>
        )}

        {/* ③ 词汇助手 */}
        {activeSection === "vocab" && (
          <section className="settings-card" aria-labelledby="vocab-heading">
            <h2 id="vocab-heading">词汇助手</h2>
          <p className="settings-hint">
            在对话窗右上角点「词汇」进入词汇助手，输入英文术语即可按标准词条模板收录。
            这里设置它默认工作的大环境（领域），留空使用内置的 Agent-LLM 开发领域。
          </p>
          <label className="field">
            <span>词汇助手大环境</span>
            <textarea
              aria-label="词汇助手大环境"
              disabled={vocabInFlight}
              maxLength={500}
              rows={3}
              placeholder="例如：Agent-LLM 开发、汽车电子、金融风控……"
              value={vocabEnv}
              onChange={(event) => setVocabEnv(event.target.value)}
            />
            <small>最多 500 字；词汇对话与普通聊天记忆互相隔离。</small>
          </label>
          <div className="button-row">
            <button
              className="button primary"
              disabled={vocabInFlight || settingsInFlight}
              type="button"
              onClick={() => void saveVocab()}
            >
              保存词汇助手设置
            </button>
            <button
              className="button danger-button"
              type="button"
              onClick={() => setConfirmingClearVocab(true)}
            >
              清除词汇词库
            </button>
          </div>
          </section>
        )}

        {/* ④ 文件归档 */}
        {activeSection === "archive" && archive && (
          <section className="settings-card" aria-labelledby="archive-heading">
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

        <div className="secondary-actions">
          <button className="text-button danger" type="button" onClick={() => setConfirmingClear(true)}>清除记忆</button>
          <button className="text-button danger" type="button" onClick={() => setConfirmingClearVocab(true)}>清除词汇对话</button>
          <button className="text-button danger" type="button" onClick={() => void exitApp()}>退出 AIbb</button>
        </div>
        <footer className="settings-footer">
          AIbb v{appVersion ?? "…"} · 本地优先 · 对话、记忆与归档记录只保存在这台电脑上
          <span className="settings-copyright">© 2026 Clink AI · 保留所有权利 · 仅限个人测试使用</span>
        </footer>
      </form>
      {profileError && <p className="feedback error" role="alert">{profileError.message}</p>}
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
      {confirmingClearVocab && (
        <section className="confirm-dialog" role="dialog" aria-label="确认清除词汇词库" aria-modal="true">
          <div className="confirm-card">
            <h2>清除词汇词库？</h2>
            <p>这会删除词汇助手的全部对话记录，普通聊天记忆不受影响。</p>
            <div className="button-row">
              <button className="button danger-button" type="button" onClick={() => void confirmClearVocab()}>确认清除</button>
              <button className="button secondary" type="button" onClick={() => setConfirmingClearVocab(false)}>取消</button>
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
