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
  apiConfigured: boolean;
}

export interface SaveSettings {
  apiBase: string;
  apiKey: string | null;
  model: string;
  webMode: WebMode;
  alwaysOnTop: boolean;
  autostart: boolean;
}

export interface AppErrorPayload {
  code: string;
  message: string;
}

export type InputDisposition =
  | { kind: "chatStarted"; requestId: string }
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

export interface ExplorationResult {
  items: [string, string, string, string];
  diary: string;
  sources: OutingSource[];
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
