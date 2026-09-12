export type PetStatus = "idle" | "chatting" | "exploring" | "returned" | "error";

export interface BootstrapState {
  firstRun: boolean;
  petStatus: PetStatus;
  apiConfigured: boolean;
}

export interface AibbProfile {
  name: string;
  avatarDataUrl: string | null;
  version: number;
}

export type WebMode = "auto" | "force" | "off";

export interface ApiSettings {
  apiBase: string;
  model: string;
  webMode: WebMode;
  alwaysOnTop: boolean;
  autostart: boolean;
  /** Custom personality (「性格定制」) injected into chat prompts. */
  persona: string;
  apiConfigured: boolean;
}

export interface SaveSettings {
  apiBase: string;
  apiKey: string | null;
  model: string;
  webMode: WebMode;
  alwaysOnTop: boolean;
  autostart: boolean;
  persona: string;
}

export interface AppErrorPayload {
  code: string;
  message: string;
}

export type InputDisposition =
  | { kind: "chatStarted"; requestId: string; spontaneousTaskId?: string }
  | { kind: "explorationStarted"; taskId: string };

export interface ChatDeltaEvent {
  requestId: string;
  delta: string;
}

export interface ChatCompleteEvent {
  requestId: string;
  message: string;
}

export interface ChatErrorEvent extends AppErrorPayload {
  requestId: string;
}

export type ExplorationStatus =
  | "queued"
  | "choosing"
  | "nativeSearching"
  | "publicSearching"
  | "reading"
  | "writing"
  | "correcting"
  | "completed"
  | "cancelled"
  | "interrupted"
  | "failed";

export interface ExplorationProgressEvent {
  taskId: string;
  status: ExplorationStatus;
}

export interface OutingSource {
  title: string;
  url: string;
}

export interface ExplorationImage {
  title: string;
  pageUrl: string;
  /** Embedded picture (data URL); the renderer never contacts the origin host. */
  dataUrl: string;
}

export interface ExplorationResult {
  items: [string, string, string, string];
  diary: string;
  sources: OutingSource[];
  images: ExplorationImage[];
  roundNumber: number;
  elapsedSeconds: number;
  rawResponse: string;
}

export type OutingTimelineMessage =
  | {
      id: string;
      role: "assistant";
      kind: "outingStatus";
      taskId: string;
      content: string;
    }
  | {
      id: string;
      role: "assistant";
      kind: "outingDiary";
      taskId: string;
      content: string;
      sources: OutingSource[];
      images: ExplorationImage[];
      roundNumber: number;
      elapsedSeconds: number;
    }
  | {
      id: string;
      role: "assistant";
      kind: "outingError";
      taskId: string;
      content: string;
    };

export interface ExplorationCompleteEvent {
  taskId: string;
  result: ExplorationResult;
}

export interface ExplorationErrorEvent extends AppErrorPayload {
  taskId: string;
}

// ── Archive (drag a file onto AIbb) ──────────────────────────────────────

export interface CategoryRule {
  name: string;
  keywords: string[];
}

export interface StructureTemplate {
  name: string;
  categories: CategoryRule[];
  hierarchy: string[];
  includeSource: boolean;
}

export interface ArchiveSettings {
  root: string;
  autoDiscover: boolean;
  templateName: string;
  templates: StructureTemplate[];
}

export interface SaveArchiveSettings {
  root: string;
  autoDiscover: boolean;
  templateName: string;
  templates: StructureTemplate[];
}

export interface DiscoveredStructure {
  hierarchy: string[];
  categories: CategoryRule[];
  detectedFrom: string | null;
}

export interface ArchiveFileResult {
  fileName: string;
  ok: boolean;
  duplicate: boolean;
  reason: string | null;
  project: string | null;
  category: string | null;
  period: string | null;
  version: string | null;
  archiveRel: string | null;
  archiveAbs: string | null;
  backupRel: string | null;
}

export interface ArchiveLedgerEntry {
  id: string;
  fileName: string;
  project: string;
  category: string;
  period: string;
  version: string;
  archiveRelPath: string;
  backupRelPath: string | null;
  status: string;
  errorCode: string | null;
  createdAt: number;
}
