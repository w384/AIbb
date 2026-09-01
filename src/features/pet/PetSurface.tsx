import { useEffect, useReducer, useRef, type PointerEvent as ReactPointerEvent } from "react";
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
  const [petState, dispatchPet] = useReducer(petStatusReducer, {
    ...initialPetState,
    status,
  });

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
  ) {
    transition({ type: "pointerEnd", pointerId: event.pointerId });
    if (
      releaseCapture &&
      event.currentTarget.hasPointerCapture?.(event.pointerId)
    ) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  async function handleClick() {
    if (gesture.current.suppressClick) {
      transition({ type: "clickConsumed" });
      return;
    }
    await toggleChatWindow();
    if (petState.status === "returned") {
      dispatchPet({ type: "RESET_TO_IDLE" });
    }
  }

  return (
    <main className="pet-surface" data-status={petState.status}>
      {petState.status === "returned" && (
        <p className="pet-bubble" role="status">
          我回来啦，点我看结果！
        </p>
      )}
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
        onPointerUp={(event) => finishPointer(event, true)}
        onPointerCancel={(event) => finishPointer(event, true)}
        onLostPointerCapture={(event) => finishPointer(event, false)}
      >
        <span aria-hidden="true">🤖</span>
      </button>
    </main>
  );
}

function isActiveExploration(status: ExplorationStatus): boolean {
  return !["completed", "cancelled", "interrupted", "failed"].includes(status);
}
