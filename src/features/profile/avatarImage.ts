const MAX_AVATAR_BYTES = 5 * 1024 * 1024;
const AVATAR_SIZE = 256;
const SUPPORTED_MIME_TYPES = new Set(["image/png", "image/jpeg", "image/webp"]);

export interface NormalizedAvatarImage {
  bytes: Uint8Array;
  mimeType: "image/webp";
}

export async function normalizeAvatarFile(file: File): Promise<NormalizedAvatarImage> {
  if (!SUPPORTED_MIME_TYPES.has(file.type)) {
    throw new Error("头像仅支持 PNG、JPEG 或 WebP 格式。");
  }
  if (file.size > MAX_AVATAR_BYTES) {
    throw new Error("头像文件不能超过 5 MiB。");
  }

  let bitmap: ImageBitmap | null = null;
  try {
    bitmap = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = AVATAR_SIZE;
    canvas.height = AVATAR_SIZE;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("canvas unavailable");

    const cropSize = Math.min(bitmap.width, bitmap.height);
    const sourceX = (bitmap.width - cropSize) / 2;
    const sourceY = (bitmap.height - cropSize) / 2;
    context.drawImage(
      bitmap,
      sourceX,
      sourceY,
      cropSize,
      cropSize,
      0,
      0,
      AVATAR_SIZE,
      AVATAR_SIZE,
    );

    const blob = await canvasToWebp(canvas);
    const bytes = new Uint8Array(await blob.arrayBuffer());
    if (bytes.length === 0 || bytes.length > MAX_AVATAR_BYTES) {
      throw new Error("invalid generated image");
    }
    return { bytes, mimeType: "image/webp" };
  } catch (reason) {
    if (reason instanceof Error && /PNG、JPEG 或 WebP|不能超过 5 MiB/.test(reason.message)) {
      throw reason;
    }
    throw new Error("头像处理失败，请换一张图片后重试。");
  } finally {
    bitmap?.close();
  }
}

function canvasToWebp(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("webp export failed"));
    }, "image/webp");
  });
}
