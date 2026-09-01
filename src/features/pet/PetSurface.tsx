import {
  useEffect,
  useReducer,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
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
  initialPetGestureState,
  initialPetState,
  petReducer,
  petStatusReducer,
  type PetGestureAction,
  type PetGestureState,
} from "./petReducer";

interface PetSurfaceProps {
  status: PetStatus;
}

export function PetSurface({ status }: PetSurfaceProps) {
  const gesture = useRef<PetGestureState>(initialPetGestureState);
  const openedOnPointerRelease = useRef(false);
  const [petState, dispatchPet] = useReducer(petStatusReducer, {
    ...initialPetState,
    status,
  });
  const [chatOpenError, setChatOpenError] = useState<string | null>(null);

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
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  function transition(action: PetGestureAction): PetGestureState {
    gesture.current = petReducer(gesture.current, action);
    return gesture.current;
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLButtonElement>) {
    if (!event.isPrimary || event.button !== 0) {
      return;
    }
    openedOnPointerRelease.current = false;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    transition({
      type: "pointerDown",
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
    });
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLButtonElement>) {
    const wasDragging = gesture.current.dragging;
    const next = transition({
      type: "pointerMove",
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      leftButtonPressed: (event.buttons & 1) === 1,
    });
    if (!wasDragging && next.dragging) {
      void startPetDrag();
    }
  }

  function finishPointer(
    event: ReactPointerEvent<HTMLButtonElement>,
    releaseCapture: boolean,
    opensChat: boolean,
  ) {
    const trackedPrimaryPointer = gesture.current.pointerId === event.pointerId;
    const wasDragging = gesture.current.dragging;
    transition({ type: "pointerEnd", pointerId: event.pointerId });
    if (
      releaseCapture &&
      event.currentTarget.hasPointerCapture?.(event.pointerId)
    ) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (opensChat && trackedPrimaryPointer && !wasDragging) {
      openedOnPointerRelease.current = true;
      void openChat();
    }
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

  function handleClick() {
    if (openedOnPointerRelease.current) {
      openedOnPointerRelease.current = false;
      return;
    }
    if (gesture.current.suppressClick) {
      transition({ type: "clickConsumed" });
      return;
    }
    void openChat();
  }

  return (
    <main className="pet-surface" data-status={petState.status}>
      {petState.status === "returned" && (
        <p className="pet-bubble" role="status">
          我回来啦，点我看结果！
        </p>
      )}
      {chatOpenError && <p className="pet-error" role="alert">{chatOpenError}</p>}
      <button
        aria-label="AIbb"
        className="pet"
        type="button"
        onClick={() => void handleClick()}
        onContextMenu={(event) => {
          event.preventDefault();
          void openSettingsWindow();
        }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={(event) => finishPointer(event, true, true)}
        onPointerCancel={(event) => finishPointer(event, true, false)}
        onLostPointerCapture={(event) => finishPointer(event, false, false)}
      >
        <span aria-hidden="true">🤖</span>
      </button>
    </main>
  );
}

function isActiveExploration(status: ExplorationStatus): boolean {
  return !["completed", "cancelled", "interrupted", "failed"].includes(status);
}
