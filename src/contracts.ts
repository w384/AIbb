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
  /** Model-chosen spots inside the diary that show a source link or picture. */
  highlights?: DiaryHighlight[];
}

/** One spot inside the diary where AIbb points the reader at something she
 * actually saw: `paragraph` is the zero-based paragraph index (paragraphs
 * are split on blank lines), and at least one of `sourceIndex` / `imageIndex`
 * is present, referencing `sources` / `images` of the same outing. */
export interface DiaryHighlight {
  paragraph: number;
  sourceIndex?: number | null;
  imageIndex?: number | null;
}

/** One stored conversation message replayed when the chat window reopens. */
export interface HistoryMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: number;
}

/** A finished outing rebuilt as a diary card after a restart. */
export interface CompletedOuting {
  roundNumber: number;
  direction: string | null;
  diary: string;
  sources: OutingSource[];
  images: ExplorationImage[];
  elapsedSeconds: number;
  createdAt: number;
}

export interface ChatHistory {
  messages: HistoryMessage[];
  outings: CompletedOuting[];
}

/** How many finished outings went to one direction. */
export interface DirectionCount {
  direction: string;
  count: number;
}

/** Collection-style outing statistics (the future heat-map basis). */
export interface OutingStats {
  totalOutings: number;
  totalDirections: number;
  lastOutingAt: number | null;
  directions: DirectionCount[];
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
      /** Model-chosen spots inside the diary that show a source link or picture. */
      highlights?: DiaryHighlight[];
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

/** Live diary chunks while AIbb writes the outing (streamed, live feel). */
export interface ExplorationDiaryDeltaEvent {
  taskId: string;
  delta: string;
}

/** The search query the model chose, shown while public pages are found. */
export interface ExplorationQueryEvent {
  taskId: string;
  query: string;
}

/** One page AIbb read, shown as it is fetched so the wait feels active. */
export interface ExplorationPageReadEvent {
  taskId: string;
  title: string;
  url: string;
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
