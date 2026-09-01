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

  it("shows stable error codes from save and connection test failures", async () => {
    vi.mocked(saveSettings).mockRejectedValue({ code: "settingsRollbackFailed", message: "safe" });
    vi.mocked(testConnection).mockRejectedValue({
      code: "authentication_failed",
      message: "safe",
    });
    render(<SettingsPanel />);
    await screen.findByLabelText("API 地址");

    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("settingsRollbackFailed");

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("authentication_failed");
  });

  it("treats a blank replacement key as keep-existing and clears the field after save", async () => {
    vi.mocked(saveSettings).mockResolvedValue();
    render(<SettingsPanel />);
    await screen.findByLabelText("API 地址");

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ apiKey: null }),
      ),
    );
    expect(screen.getByLabelText("API Key")).toHaveValue("");
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
