import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ArchivePanel } from "./ArchivePanel";
import {
  archiveFiles,
  archiveLedger,
  takePendingArchivePaths,
} from "../../lib/tauri";

let pendingListener: (() => void) | undefined;

vi.mock("../../lib/tauri", () => ({
  takePendingArchivePaths: vi.fn(),
  archiveLedger: vi.fn(),
  archiveFiles: vi.fn(),
  listenArchivePending: vi.fn(async (listener: () => void) => {
    pendingListener = listener;
    return () => {};
  }),
}));

const mockTake = vi.mocked(takePendingArchivePaths);
const mockArchive = vi.mocked(archiveFiles);
const mockLedger = vi.mocked(archiveLedger);

describe("ArchivePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockTake.mockResolvedValue([]);
    mockLedger.mockResolvedValue([]);
  });

  it("renders nothing when no files were dropped", async () => {
    const { container } = render(<ArchivePanel />);
    await waitFor(() => expect(mockTake).toHaveBeenCalledTimes(1));
    expect(container.firstChild).toBeNull();
  });

  it("shows pending files when the host announces a drop while the chat window is already open", async () => {
    render(<ArchivePanel />);
    await waitFor(() => expect(pendingListener).toBeDefined());

    mockTake.mockResolvedValueOnce(["C:\\drop\\新合同v0.1.pdf"]);
    act(() => pendingListener!());

    expect(await screen.findByText("新合同v0.1.pdf")).toBeInTheDocument();
    expect(screen.getByText("1 个文件等待归档")).toBeInTheDocument();
  });

  it("lists dropped files and archives them under the entered project", async () => {
    mockTake.mockResolvedValue(["C:\\drop\\集成方案v0.1.pdf"]);
    mockArchive.mockResolvedValue([
      {
        fileName: "集成方案v0.1.pdf",
        ok: true,
        duplicate: false,
        reason: null,
        project: "0828",
        category: "方案",
        period: "202608w5",
        version: "v0.1",
        archiveRel: "0828/202608w5/方案/集成方案v0.1.pdf",
        archiveAbs: null,
        backupRel: "0828/源文件/202608w5/集成方案v0.1.pdf",
      },
    ]);

    render(<ArchivePanel />);
    expect(await screen.findByText("集成方案v0.1.pdf")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: /归档到项目/ }), {
      target: { value: "0828" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始归档" }));

    await waitFor(() =>
      expect(mockArchive).toHaveBeenCalledWith(
        ["C:\\drop\\集成方案v0.1.pdf"],
        "0828",
      ),
    );
    expect(await screen.findByText(/已归档到/)).toBeInTheDocument();
  });

  it("explains a duplicate outcome without archiving again", async () => {
    mockTake.mockResolvedValue(["C:\\drop\\合同v0.1.docx"]);
    mockArchive.mockResolvedValue([
      {
        fileName: "合同v0.1.docx",
        ok: false,
        duplicate: true,
        reason: "归档区已存在完全相同内容的文件，为避免重复归档已拒绝。",
        project: null,
        category: null,
        period: null,
        version: null,
        archiveRel: "A/202608w5/合同/合同v0.1.docx",
        archiveAbs: null,
        backupRel: null,
      },
    ]);

    render(<ArchivePanel />);
    expect(await screen.findByText("合同v0.1.docx")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: /归档到项目/ }), {
      target: { value: "A" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始归档" }));

    expect(await screen.findByText(/重复归档/)).toBeInTheDocument();
  });

  it("rejects a project name containing path separators", async () => {
    mockTake.mockResolvedValue(["C:\\drop\\方案.pdf"]);

    render(<ArchivePanel />);
    expect(await screen.findByText("方案.pdf")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: /归档到项目/ }), {
      target: { value: "../outside" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始归档" }));

    expect(await screen.findByText(/路径分隔符/)).toBeInTheDocument();
    expect(mockArchive).not.toHaveBeenCalled();
  });

  it("dismisses and hides the panel after archiving", async () => {
    mockTake.mockResolvedValue(["C:\\drop\\资料.txt"]);
    mockArchive.mockResolvedValue([
      {
        fileName: "资料.txt",
        ok: true,
        duplicate: false,
        reason: null,
        project: "P",
        category: "其他",
        period: "202608w5",
        version: "v0.1",
        archiveRel: "P/202608w5/其他/资料v0.1.txt",
        archiveAbs: null,
        backupRel: null,
      },
    ]);

    const { container } = render(<ArchivePanel />);
    expect(await screen.findByText("资料.txt")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: /归档到项目/ }), {
      target: { value: "P" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始归档" }));
    await screen.findByText(/已归档到/);

    fireEvent.click(screen.getByRole("button", { name: "收起归档面板" }));
    await waitFor(() => expect(container.firstChild).toBeNull());
  });
});
