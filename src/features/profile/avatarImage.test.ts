import { afterEach, describe, expect, it, vi } from "vitest";
import { normalizeAvatarFile } from "./avatarImage";

const pngBytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

describe("normalizeAvatarFile", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("normalizes a local PNG to a square WebP without preserving its source name", async () => {
    const bitmap = { width: 800, height: 400, close: vi.fn() };
    const drawImage = vi.fn();
    vi.stubGlobal("createImageBitmap", vi.fn().mockResolvedValue(bitmap));
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      drawImage,
    } as unknown as CanvasRenderingContext2D);
    vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => {
      callback(new Blob([new Uint8Array([1, 2, 3])], { type: "image/webp" }));
    });

    const result = await normalizeAvatarFile(
      new File([pngBytes], "C:\\private\\face.png", { type: "image/png" }),
    );

    expect(result.mimeType).toBe("image/webp");
    expect(result.bytes).toEqual(new Uint8Array([1, 2, 3]));
    expect(drawImage).toHaveBeenCalledWith(bitmap, 200, 0, 400, 400, 0, 0, 256, 256);
    expect(bitmap.close).toHaveBeenCalledTimes(1);
  });

  it("rejects an unsupported or oversized file before trying to decode it", async () => {
    const createBitmap = vi.fn();
    vi.stubGlobal("createImageBitmap", createBitmap);

    await expect(
      normalizeAvatarFile(new File(["not an image"], "avatar.gif", { type: "image/gif" })),
    ).rejects.toThrow("PNG、JPEG 或 WebP");
    await expect(
      normalizeAvatarFile(new File([new Uint8Array(5 * 1024 * 1024 + 1)], "large.png", { type: "image/png" })),
    ).rejects.toThrow("不能超过 5 MiB");

    expect(createBitmap).not.toHaveBeenCalled();
  });
});
