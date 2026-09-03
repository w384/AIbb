import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockListen } = vi.hoisted(() => ({ mockListen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mockListen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }));

import { listenProfileUpdated } from "./tauri";

describe("listenProfileUpdated", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("forwards the profile update payload from Tauri", async () => {
    const listener = vi.fn();
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    await expect(listenProfileUpdated(listener)).resolves.toBe(unlisten);

    expect(mockListen).toHaveBeenCalledWith("profile://updated", expect.any(Function));
    const tauriListener = mockListen.mock.calls[0]?.[1] as (event: {
      payload: { name: string; avatarDataUrl: string | null; version: number };
    }) => void;
    tauriListener({
      payload: { name: "小团子", avatarDataUrl: "data:image/webp;base64,AQID", version: 3 },
    });
    expect(listener).toHaveBeenCalledWith({
      name: "小团子",
      avatarDataUrl: "data:image/webp;base64,AQID",
      version: 3,
    });
  });
});
