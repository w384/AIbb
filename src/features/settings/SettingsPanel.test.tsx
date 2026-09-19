import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";
import {
  clearMemory,
  clearVocabMemory,
  exitApp,
  listAvailableModels,
  loadAibbProfile,
  listenProfileUpdated,
  loadSettings,
  resetAibbAvatar,
  saveAibbAvatar,
  saveAibbName,
  saveSettings,
  testConnection,
} from "../../lib/tauri";
import { normalizeAvatarFile } from "../profile/avatarImage";

let profileListener: (profile: { name: string; avatarDataUrl: string | null; version: number }) => void;
const unlistenProfile = vi.fn();

vi.mock("../../lib/tauri", () => ({
  clearMemory: vi.fn(),
  clearVocabMemory: vi.fn(),
  exitApp: vi.fn(),
  getAppVersion: vi.fn(async () => "0.3.0"),
  loadAibbProfile: vi.fn(),
  listenProfileUpdated: vi.fn(async (listener: typeof profileListener) => {
    profileListener = listener;
    return unlistenProfile;
  }),
  loadSettings: vi.fn(),
  loadArchiveSettings: vi.fn(async () => ({
    root: "",
    autoDiscover: true,
    templateName: "默认",
    templates: [
      {
        name: "默认",
        categories: [{ name: "其他", keywords: [] }],
        hierarchy: ["week", "category"],
        includeSource: true,
      },
    ],
  })),
  saveArchiveSettings: vi.fn(async () => undefined),
  discoverArchiveStructure: vi.fn(async () => null),
  resetAibbAvatar: vi.fn(),
  saveAibbAvatar: vi.fn(),
  saveAibbName: vi.fn(),
  saveSettings: vi.fn(),
  testConnection: vi.fn(),
  listAvailableModels: vi.fn(),
}));

vi.mock("../profile/avatarImage", () => ({
  normalizeAvatarFile: vi.fn(),
}));

describe("SettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(saveSettings).mockResolvedValue();
    vi.mocked(testConnection).mockResolvedValue();
    vi.mocked(clearMemory).mockResolvedValue();
    vi.mocked(clearVocabMemory).mockResolvedValue();
    vi.mocked(exitApp).mockResolvedValue();
    vi.mocked(normalizeAvatarFile).mockResolvedValue({
      bytes: new Uint8Array([1, 2, 3]),
      mimeType: "image/webp",
    });
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: null,
      version: 0,
    });
    vi.mocked(resetAibbAvatar).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: null,
      version: 1,
    });
    vi.mocked(saveAibbAvatar).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: "data:image/webp;base64,AQID",
      version: 1,
    });
    vi.mocked(saveAibbName).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: null,
      version: 1,
    });
    vi.mocked(loadSettings).mockResolvedValue({
      apiBase: "https://example.test/v1",
      model: "m",
      webMode: "auto",
      alwaysOnTop: true,
      autostart: false,
      persona: "",
      vocabEnv: "",
      apiConfigured: true,
    });
  });

  it("saves a trimmed nickname and previews it", async () => {
    render(<SettingsPanel />);
    fireEvent.change(await screen.findByLabelText("AIbb 昵称"), { target: { value: " 小团子 " } });

    expect(screen.getByText("小团子")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "保存 AIbb 资料" }));

    await waitFor(() => expect(saveAibbName).toHaveBeenCalledWith("小团子"));
  });

  async function openTab(label: string) {
    fireEvent.click(await screen.findByRole("button", { name: label }));
  }

  it("accepts only a newer external profile update", async () => {
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "本地资料",
      avatarDataUrl: null,
      version: 3,
    });
    render(<SettingsPanel />);

    await screen.findByLabelText("AIbb 昵称");
    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    act(() => profileListener({
      name: "旧资料",
      avatarDataUrl: null,
      version: 3,
    }));
    expect(screen.getByLabelText("AIbb 昵称")).toHaveValue("本地资料");

    act(() => profileListener({
      name: "小团子",
      avatarDataUrl: "data:image/webp;base64,AA==",
      version: 4,
    }));
    expect(screen.getByLabelText("AIbb 昵称")).toHaveValue("小团子");
    expect(screen.getByRole("img", { name: "小团子" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AA==",
    );
  });

  it("unregisters its profile listener on unmount", async () => {
    const view = render(<SettingsPanel />);

    await screen.findByLabelText("AIbb 昵称");
    await waitFor(() => expect(listenProfileUpdated).toHaveBeenCalledTimes(1));
    view.unmount();

    expect(unlistenProfile).toHaveBeenCalledTimes(1);
  });

  it("handles profile-listener registration failure without an unhandled rejection", async () => {
    vi.mocked(listenProfileUpdated).mockRejectedValueOnce({
      code: "profileStorageUnavailable",
      message: "native details must stay hidden",
    });

    render(<SettingsPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法访问 AIbb 头像，请稍后再试。",
    );
    expect(screen.queryByText(/native details/)).not.toBeInTheDocument();
  });

  it("does not change the displayed or persisted nickname when avatar save fails", async () => {
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "原名",
      avatarDataUrl: null,
      version: 2,
    });
    vi.mocked(saveAibbAvatar).mockRejectedValue({ code: "profileStorageUnavailable" });
    render(<SettingsPanel />);

    fireEvent.change(await screen.findByLabelText("AIbb 昵称"), { target: { value: "新名字" } });
    fireEvent.change(screen.getByLabelText("选择头像"), {
      target: { files: [new File(["png"], "face.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(normalizeAvatarFile).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "保存 AIbb 资料" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("无法访问 AIbb 头像");
    expect(saveAibbName).not.toHaveBeenCalled();
    expect(screen.getByLabelText("AIbb 昵称")).toHaveValue("原名");
    expect(screen.getByText("原名")).toBeVisible();
  });

  it("reports the saved avatar when nickname save fails after it", async () => {
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "原名",
      avatarDataUrl: null,
      version: 2,
    });
    vi.mocked(saveAibbAvatar).mockResolvedValue({
      name: "原名",
      avatarDataUrl: "data:image/webp;base64,AQID",
      version: 3,
    });
    vi.mocked(saveAibbName).mockRejectedValue({ code: "invalidProfile" });
    render(<SettingsPanel />);

    fireEvent.change(await screen.findByLabelText("AIbb 昵称"), { target: { value: "新名字" } });
    fireEvent.change(screen.getByLabelText("选择头像"), {
      target: { files: [new File(["png"], "face.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(normalizeAvatarFile).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "保存 AIbb 资料" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("头像已保存，昵称未保存");
    expect(screen.getByLabelText("AIbb 昵称")).toHaveValue("原名");
    expect(screen.getByRole("img", { name: "原名" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AQID",
    );
  });

  it("keeps API settings available when only the stored profile cannot load", async () => {
    vi.mocked(loadAibbProfile).mockRejectedValue({ code: "profileStorageUnavailable" });
    render(<SettingsPanel />);
    await openTab("API 设置");

    expect(await screen.findByLabelText("API 地址")).toHaveValue("https://example.test/v1");
    expect(await screen.findByText("无法访问 AIbb 头像，请稍后再试。")).toBeVisible();
  });

  it("keeps profile load feedback visible when saving unrelated API settings", async () => {
    vi.mocked(loadAibbProfile).mockRejectedValue({ code: "profileStorageUnavailable" });
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByText("无法访问 AIbb 头像，请稍后再试。");

    fireEvent.click(screen.getByRole("button", { name: "仅保存" }));

    await waitFor(() => expect(saveSettings).toHaveBeenCalledTimes(1));
    expect(screen.getByText("无法访问 AIbb 头像，请稍后再试。")).toBeVisible();
  });

  it("allows all 24 Unicode nickname scalars instead of limiting UTF-16 code units", async () => {
    render(<SettingsPanel />);
    const nickname = await screen.findByLabelText("AIbb 昵称");
    const name = "😀".repeat(24);

    fireEvent.change(nickname, { target: { value: name } });

    expect(nickname).toHaveValue(name);
  });

  it("keeps the current preview when a selected image cannot be normalized", async () => {
    vi.mocked(normalizeAvatarFile).mockRejectedValue(new Error("bad image"));
    vi.mocked(loadAibbProfile).mockResolvedValue({
      name: "AIbb",
      avatarDataUrl: "data:image/webp;base64,OLD",
      version: 2,
    });
    render(<SettingsPanel />);

    const image = await screen.findByRole("img", { name: "AIbb" });
    fireEvent.change(screen.getByLabelText("选择头像"), {
      target: { files: [new File(["not an image"], "bad.gif", { type: "image/gif" })] },
    });

    expect(await screen.findByRole("alert")).toHaveTextContent("头像处理失败");
    expect(image).toHaveAttribute("src", "data:image/webp;base64,OLD");
    expect(saveAibbAvatar).not.toHaveBeenCalled();
  });

  it("loads only non-secret settings and keeps the replacement API key empty", async () => {
    render(<SettingsPanel />);
    await openTab("API 设置");

    expect(await screen.findByLabelText("API 地址")).toHaveValue("https://example.test/v1");
    expect(screen.getByLabelText("API Key")).toHaveValue("");
    expect(screen.getByLabelText("模型名称")).toHaveValue("m");
    expect(screen.getByLabelText("联网模式")).toHaveValue("auto");
    expect(screen.getByLabelText("始终置顶")).toBeChecked();
    expect(screen.getByLabelText("开机启动")).not.toBeChecked();
  });

  it("turns a blank DeepSeek model hint into the actual saved default", async () => {
    vi.mocked(loadSettings).mockResolvedValue({
      apiBase: "https://api.deepseek.com",
      model: "",
      webMode: "auto",
      alwaysOnTop: true,
      autostart: false,
      persona: "",
      vocabEnv: "",
      apiConfigured: true,
    });
    render(<SettingsPanel />);
    await openTab("API 设置");

    const model = await screen.findByLabelText("模型名称");
    expect(model).toHaveValue("deepseek-v4-flash");
    expect(model).toBeRequired();
    fireEvent.click(screen.getByRole("button", { name: "仅保存" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ model: "deepseek-v4-flash" }),
      ),
    );
  });

  it("shows a Chinese authentication error without provider internals", async () => {
    vi.mocked(saveSettings).mockResolvedValue();
    vi.mocked(testConnection).mockRejectedValue({
      code: "authentication_failed",
      message: "The model provider rejected the API credential.",
    });
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "保存并测试" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("API Key");
    expect(alert).toHaveTextContent("重新输入");
    expect(alert).not.toHaveTextContent("authentication_failed");
    expect(alert).not.toHaveTextContent("model provider");
  });

  it("saves the current API key before testing the connection", async () => {
    let finishSaving: (() => void) | undefined;
    vi.mocked(saveSettings).mockImplementation(
      () => new Promise<void>((resolve) => {
        finishSaving = resolve;
      }),
    );
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://api.deepseek.com" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "  current-key  " },
    });
    fireEvent.change(screen.getByLabelText("模型名称"), {
      target: { value: "deepseek-v4-flash" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存并测试" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          apiBase: "https://api.deepseek.com",
          apiKey: "current-key",
          model: "deepseek-v4-flash",
        }),
      ),
    );
    expect(testConnection).not.toHaveBeenCalled();
    expect(screen.getByLabelText("API Key")).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存并测试" })).toBeDisabled();

    finishSaving?.();

    await waitFor(() => expect(testConnection).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("status")).toHaveTextContent("连接成功");
  });

  it("does not test the connection when saving the current settings fails", async () => {
    vi.mocked(saveSettings).mockRejectedValue({
      code: "credentialStoreUnavailable",
      message: "provider internal detail",
    });
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "保存并测试" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("系统凭据服务");
    expect(testConnection).not.toHaveBeenCalled();
  });

  it("lists the provider models and fills the selected one into the model field", async () => {
    vi.mocked(listAvailableModels).mockResolvedValue([
      "deepseek-v4-flash",
      "deepseek-v4-pro",
    ]);
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://api.deepseek.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "检查可用模型" }));

    expect(await screen.findByText("deepseek-v4-pro")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "deepseek-v4-pro" }));

    expect(screen.getByLabelText("模型名称")).toHaveValue("deepseek-v4-pro");
  });

  it("warns when the current model is missing from the provider list", async () => {
    vi.mocked(listAvailableModels).mockResolvedValue([
      "deepseek-v4-flash",
      "deepseek-v4-pro",
    ]);
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://api.deepseek.com" },
    });
    fireEvent.change(screen.getByLabelText("模型名称"), {
      target: { value: "deepseek-chat" },
    });
    fireEvent.click(screen.getByRole("button", { name: "检查可用模型" }));

    expect(await screen.findByText(/不在该 API 的可用列表里/)).toBeInTheDocument();
  });

  it("explains that an empty API key keeps the saved credential", async () => {
    render(<SettingsPanel />);
    await openTab("API 设置");

    await screen.findByLabelText("API 地址");

    expect(screen.getByText("留空表示继续使用已保存的密钥")).toBeVisible();
  });

  it("treats a blank replacement key as keep-existing and clears the field after save", async () => {
    vi.mocked(saveSettings).mockResolvedValue();
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "仅保存" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ apiKey: null }),
      ),
    );
    expect(screen.getByLabelText("API Key")).toHaveValue("");
  });

  it("shows that a newly entered API key is saved after a successful save", async () => {
    vi.mocked(loadSettings).mockResolvedValue({
      apiBase: "https://api.deepseek.com",
      model: "deepseek-v4-flash",
      webMode: "auto",
      alwaysOnTop: true,
      autostart: false,
      persona: "",
      vocabEnv: "",
      apiConfigured: false,
    });
    render(<SettingsPanel />);
    await openTab("API 设置");
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "new-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "仅保存" }));

    await waitFor(() => expect(screen.getByLabelText("API Key")).toHaveValue(""));
    expect(screen.getByLabelText("API Key")).toHaveAttribute("placeholder", "已安全保存");
  });

  it("saves a chosen personality preset with the basic profile", async () => {
    render(<SettingsPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "活泼元气" }));

    const personaField = screen.getByLabelText("自定义性格设定") as HTMLTextAreaElement;
    expect(personaField.value).toContain("元气满满");
    fireEvent.click(screen.getByRole("button", { name: "保存 AIbb 资料" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ persona: expect.stringContaining("元气满满") }),
      ),
    );
  });

  it("requires an in-window confirmation before clearing memory", async () => {
    render(<SettingsPanel />);
    await screen.findByRole("heading", { name: "AIbb 设置" });

    fireEvent.click(screen.getByRole("button", { name: "清除记忆" }));
    expect(clearMemory).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "确认清除记忆" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认清除" }));
    await waitFor(() => expect(clearMemory).toHaveBeenCalledTimes(1));
  });

  it("saves the vocabulary environment from its own section", async () => {
    render(<SettingsPanel />);
    await openTab("词汇助手");

    fireEvent.change(screen.getByLabelText("词汇助手大环境"), {
      target: { value: " 汽车电子开发 " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存词汇助手设置" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ vocabEnv: "汽车电子开发" }),
      ),
    );
  });

  it("clears only the vocabulary channel after an in-window confirmation", async () => {
    render(<SettingsPanel />);
    await openTab("词汇助手");

    fireEvent.click(screen.getByRole("button", { name: "清除词汇词库" }));
    expect(clearVocabMemory).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "确认清除词汇词库" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认清除" }));
    await waitFor(() => expect(clearVocabMemory).toHaveBeenCalledTimes(1));
    expect(clearMemory).not.toHaveBeenCalled();
  });

  it("clears the vocabulary conversation from the bottom actions next to 清除记忆", async () => {
    render(<SettingsPanel />);
    await screen.findByRole("heading", { name: "AIbb 设置" });

    fireEvent.click(screen.getByRole("button", { name: "清除词汇对话" }));
    expect(clearVocabMemory).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "确认清除词汇词库" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认清除" }));
    await waitFor(() => expect(clearVocabMemory).toHaveBeenCalledTimes(1));
    expect(clearMemory).not.toHaveBeenCalled();
  });

  it("offers an explicit exit action from settings", async () => {
    render(<SettingsPanel />);
    await screen.findByRole("heading", { name: "AIbb 设置" });

    fireEvent.click(screen.getByRole("button", { name: "退出 AIbb" }));

    expect(exitApp).toHaveBeenCalledTimes(1);
  });

  it("shows the app version from the backend in the footer", async () => {
    render(<SettingsPanel />);
    await screen.findByRole("heading", { name: "AIbb 设置" });

    expect(await screen.findByText(/AIbb v0\.3\.0/)).toBeVisible();
  });
});
