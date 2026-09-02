import {
  useEffect,
  useReducer,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { AibbAvatar } from "../../components/AibbAvatar";
import type {
  ExplorationStatus,
  PetStatus,
} from "../../contracts";
import {
  listenExplorationComplete,
  listenExplorationError,
  listenExplorationProgress,
  openSettingsWindow,
  startPetDrag,
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
  const longPressTimer = useRef<number | null>(null);
  const activePointer = useRef<number | null>(null);
  const pressOrigin = useRef<PressOrigin | null>(null);
  const suppressNextClick = useRef(false);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    void Promise.all([
      listenExplorationProgress((event) => {
        if (isActiveExploration(event.status)) {
          dispatchPet({ type: "EXPLORATION_STARTED", taskId: event.taskId });
        }
      }),
      listenExplorationComplete((event) => {
        dispatchPet({ type: "EXPLORATION_COMPLETED", taskId: event.taskId });
      }),
      listenExplorationError((event) => {
        dispatchPet({ type: "EXPLORATION_FAILED", taskId: event.taskId });
      }),
    ]).then((installed) => {
      if (disposed) installed.forEach((unlisten) => unlisten());
      else unlisteners.push(...installed);
    });
    return () => {
      disposed = true;
      clearLongPressTimer();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  function clearLongPressTimer() {
    if (longPressTimer.current !== null) {
      window.clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
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
    activePointer.current = null;
    pressOrigin.current = null;
    suppressNextClick.current = true;
    void startPetDrag();
  }

  function finishPointer(event: ReactPointerEvent<HTMLButtonElement>) {
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
    <main className="pet-surface" data-status={petState.status}>
      {petState.status === "returned" && (
        <span className="pet-return-indicator" role="status">
          <span className="sr-only">我回来啦，点我看结果！</span>
        </span>
      )}
      {chatOpenError && <p className="pet-error" role="alert">{chatOpenError}</p>}
      <button
        aria-label="AIbb"
        className="pet"
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
        <AibbAvatar />
      </button>
    </main>
  );
}

function isActiveExploration(status: ExplorationStatus): boolean {
  return !["completed", "cancelled", "interrupted", "failed"].includes(status);
}
