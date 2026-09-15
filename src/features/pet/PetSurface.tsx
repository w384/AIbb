import {
  useEffect,
  useReducer,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { AibbAvatar } from "../../components/AibbAvatar";
import type {
  AibbProfile,
  ExplorationStatus,
  PetStatus,
} from "../../contracts";
import {
  loadAibbProfile,
  listenProfileUpdated,
  listenExplorationComplete,
  listenExplorationError,
  listenExplorationProgress,
  openArchiveWindow,
  openSettingsWindow,
  petDragBegin,
  petDragEnd,
  petDragMove,
  toggleChatWindow,
} from "../../lib/tauri";
import {
  initialPetState,
  petStatusReducer,
} from "./petReducer";

interface PetSurfaceProps {
  status: PetStatus;
}

const LONG_PRESS_MS = 100;
const DRAG_DISTANCE_PX = 3;
const DEFAULT_PROFILE: AibbProfile = {
  name: "AIbb",
  avatarDataUrl: null,
  version: 0,
};

interface PressOrigin {
  pointerId: number;
  x: number;
  y: number;
}

export function PetSurface({ status }: PetSurfaceProps) {
  const [petState, dispatchPet] = useReducer(petStatusReducer, {
    ...initialPetState,
    status,
  });
  const [chatOpenError, setChatOpenError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [profile, setProfile] = useState<AibbProfile>(DEFAULT_PROFILE);
  const longPressTimer = useRef<number | null>(null);
  const activePointer = useRef<number | null>(null);
  const pressOrigin = useRef<PressOrigin | null>(null);
  const suppressNextClick = useRef(false);
  // 手动拖动：窗口跟随指针移动（保持透明），pointermove 用 rAF 节流，
  // 每帧只发一次移动指令，后端直接读系统光标位置来定位窗口，不做
  // CSS/DPI 换算，任何方向、任何缩放都不漂移。
  const dragFrame = useRef<number | null>(null);
  const draggingRef = useRef(false);
  const dragPointerId = useRef<number | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const installListener = (registration: Promise<() => void>) => {
      void registration
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlisteners.push(unlisten);
        })
        .catch(() => {});
    };
    installListener(listenProfileUpdated((updatedProfile) => {
      setProfile((current) =>
        updatedProfile.version >= current.version ? updatedProfile : current,
      );
    }));
    installListener(listenExplorationProgress((event) => {
      if (isActiveExploration(event.status)) {
        dispatchPet({ type: "EXPLORATION_STARTED", taskId: event.taskId });
      }
    }));
    installListener(listenExplorationComplete((event) => {
      dispatchPet({ type: "EXPLORATION_COMPLETED", taskId: event.taskId });
    }));
    installListener(listenExplorationError((event) => {
      dispatchPet({ type: "EXPLORATION_FAILED", taskId: event.taskId });
    }));
    installListener(getCurrentWebview().onDragDropEvent((event) => {
      const { type } = event.payload;
      if (type === "enter" || type === "over") {
        setDragOver(true);
        return;
      }
      if (type === "leave") {
        setDragOver(false);
        return;
      }
      // drop
      setDragOver(false);
      const paths = event.payload.paths;
      if (paths.length === 0) {
        setChatOpenError("没能读取拖入的文件，请再拖一次试试。");
        return;
      }
      void openArchiveWindow(paths).catch(() => {
        setChatOpenError("没能打开归档面板，请再拖一次试试。");
      });
    }));
    void loadAibbProfile()
      .then((loadedProfile) => {
        if (!disposed) {
          setProfile((current) =>
            loadedProfile.version >= current.version ? loadedProfile : current,
          );
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      clearLongPressTimer();
      cancelDragFrame();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  function clearLongPressTimer() {
    if (longPressTimer.current !== null) {
      window.clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  }

  function cancelDragFrame() {
    if (dragFrame.current !== null) {
      window.cancelAnimationFrame(dragFrame.current);
      dragFrame.current = null;
    }
  }

  function scheduleDragMove() {
    if (dragFrame.current !== null) return;
    dragFrame.current = window.requestAnimationFrame(() => {
      dragFrame.current = null;
      void petDragMove();
    });
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLButtonElement>) {
    if (!event.isPrimary || event.button !== 0) {
      return;
    }
    clearLongPressTimer();
    suppressNextClick.current = false;
    activePointer.current = event.pointerId;
    pressOrigin.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    longPressTimer.current = window.setTimeout(() => {
      beginDrag(event.pointerId);
    }, LONG_PRESS_MS);
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLButtonElement>) {
    // 拖动进行中：认正在拖动的那个指针，每帧通知后端移动一次窗口。
    if (draggingRef.current) {
      if (dragPointerId.current === event.pointerId) {
        scheduleDragMove();
      }
      return;
    }
    const origin = pressOrigin.current;
    if (
      !origin ||
      origin.pointerId !== event.pointerId ||
      activePointer.current !== event.pointerId ||
      (event.buttons & 1) === 0
    ) {
      return;
    }
    if (
      Math.hypot(event.clientX - origin.x, event.clientY - origin.y) >=
      DRAG_DISTANCE_PX
    ) {
      beginDrag(event.pointerId);
    }
  }

  function beginDrag(pointerId: number) {
    if (activePointer.current !== pointerId) return;
    clearLongPressTimer();
    dragPointerId.current = pointerId;
    activePointer.current = null;
    pressOrigin.current = null;
    suppressNextClick.current = true;
    draggingRef.current = true;
    setDragging(true);
    void petDragBegin();
  }

  function finishPointer(event: ReactPointerEvent<HTMLButtonElement>) {
    if (draggingRef.current && dragPointerId.current === event.pointerId) {
      draggingRef.current = false;
      dragPointerId.current = null;
      setDragging(false);
      cancelDragFrame();
      void petDragEnd();
    }
    if (activePointer.current !== event.pointerId) return;
    activePointer.current = null;
    pressOrigin.current = null;
    clearLongPressTimer();
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture?.(event.pointerId);
    }
  }

  function handleClick(event: ReactMouseEvent<HTMLButtonElement>) {
    if (event.detail === 0) {
      event.preventDefault();
      return;
    }
    if (suppressNextClick.current) {
      suppressNextClick.current = false;
      event.preventDefault();
      return;
    }
    void openChat();
  }

  async function openChat() {
    setChatOpenError(null);
    try {
      await toggleChatWindow();
    } catch {
      setChatOpenError("暂时没能打开对话，请再点一次试试。");
      return;
    }
    if (petState.status === "returned") {
      dispatchPet({ type: "RESET_TO_IDLE" });
    }
  }

  return (
    <main
      className={`pet-surface${dragOver ? " drop-active" : ""}`}
      data-status={petState.status}
    >
      {petState.status === "returned" && (
        <span className="pet-return-indicator" role="status">
          <span className="sr-only">我回来啦，点我看结果！</span>
        </span>
      )}
      {chatOpenError && <p className="pet-error" role="alert">{chatOpenError}</p>}
      <button
        aria-label={profile.name}
        className={`pet${dragging ? " dragging" : ""}`}
        tabIndex={-1}
        title="短按对话，长按拖动，右键设置"
        type="button"
        onClick={handleClick}
        onContextMenu={(event) => {
          event.preventDefault();
          void openSettingsWindow();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") event.preventDefault();
        }}
        onLostPointerCapture={finishPointer}
        onPointerCancel={finishPointer}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishPointer}
      >
        <AibbAvatar avatarDataUrl={profile.avatarDataUrl} name={profile.name} />
      </button>
    </main>
  );
}

function isActiveExploration(status: ExplorationStatus): boolean {
  return !["completed", "cancelled", "interrupted", "failed"].includes(status);
}
