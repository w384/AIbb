import { useRef, type PointerEvent as ReactPointerEvent } from "react";
import type { PetStatus } from "../../contracts";
import {
  openSettingsWindow,
  startPetDrag,
  toggleChatWindow,
} from "../../lib/tauri";
import {
  initialPetGestureState,
  petReducer,
  type PetGestureAction,
  type PetGestureState,
} from "./petReducer";

interface PetSurfaceProps {
  status: PetStatus;
}

export function PetSurface({ status }: PetSurfaceProps) {
  const gesture = useRef<PetGestureState>(initialPetGestureState);

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

  function handleClick() {
    if (gesture.current.suppressClick) {
      transition({ type: "clickConsumed" });
      return;
    }
    void toggleChatWindow();
  }

  return (
    <main className="pet-surface" data-status={status}>
      <button
        aria-label="AIbb"
        className="pet"
        type="button"
        onClick={handleClick}
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
