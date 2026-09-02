import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";
import {
  clearMemory,
  exitApp,
  loadSettings,
  saveSettings,
  testConnection,
} from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({
  clearMemory: vi.fn(),
  exitApp: vi.fn(),
  loadSettings: vi.fn(),
  saveSettings: vi.fn(),
  testConnection: vi.fn(),
}));

describe("SettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(saveSettings).mockResolvedValue();
    vi.mocked(testConnection).mockResolvedValue();
    vi.mocked(clearMemory).mockResolvedValue();
    vi.mocked(exitApp).mockResolvedValue();
    vi.mocked(loadSettings).mockResolvedValue({
      apiBase: "https://example.test/v1",
      model: "m",
      webMode: "auto",
      alwaysOnTop: true,
      autostart: false,
      apiConfigured: true,
    });
  });

  it("loads only non-secret settings and keeps the replacement API key empty", async () => {
    render(<SettingsPanel />);

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
      apiConfigured: true,
    });
    render(<SettingsPanel />);

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
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "保存并测试" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("系统凭据服务");
    expect(testConnection).not.toHaveBeenCalled();
  });

  it("explains that an empty API key keeps the saved credential", async () => {
    render(<SettingsPanel />);

    await screen.findByLabelText("API 地址");

    expect(screen.getByText("留空表示继续使用已保存的密钥")).toBeVisible();
  });

  it("treats a blank replacement key as keep-existing and clears the field after save", async () => {
    vi.mocked(saveSettings).mockResolvedValue();
    render(<SettingsPanel />);
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
      apiConfigured: false,
    });
    render(<SettingsPanel />);
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "new-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "仅保存" }));

    await waitFor(() => expect(screen.getByLabelText("API Key")).toHaveValue(""));
    expect(screen.getByLabelText("API Key")).toHaveAttribute("placeholder", "已安全保存");
  });

  it("requires an in-window confirmation before clearing memory", async () => {
    render(<SettingsPanel />);
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "清除记忆" }));
    expect(clearMemory).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "确认清除记忆" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认清除" }));
    await waitFor(() => expect(clearMemory).toHaveBeenCalledTimes(1));
  });

  it("offers an explicit exit action from settings", async () => {
    render(<SettingsPanel />);
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "退出 AIbb" }));

    expect(exitApp).toHaveBeenCalledTimes(1);
  });
});
