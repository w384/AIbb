import {
  useEffect,
  useReducer,
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
  initialPetState,
  petStatusReducer,
} from "./petReducer";

interface PetSurfaceProps {
  status: PetStatus;
}

export function PetSurface({ status }: PetSurfaceProps) {
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

  function handleDragPointerDown(event: ReactPointerEvent<HTMLButtonElement>) {
    if (!event.isPrimary || event.button !== 0) {
      return;
    }
    event.preventDefault();
    void startPetDrag();
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
        <p className="pet-bubble" role="status">
          我回来啦，点我看结果！
        </p>
      )}
      {chatOpenError && <p className="pet-error" role="alert">{chatOpenError}</p>}
      <button
        aria-label="移动 AIbb"
        className="pet-drag-handle"
        title="按住这里移动 AIbb"
        type="button"
        onPointerDown={handleDragPointerDown}
      >
        <span aria-hidden="true">•••</span>
      </button>
      <button
        aria-label="AIbb"
        className="pet"
        type="button"
        onClick={() => void openChat()}
        onContextMenu={(event) => {
          event.preventDefault();
          void openSettingsWindow();
        }}
      >
        <span aria-hidden="true">🤖</span>
      </button>
    </main>
  );
}

function isActiveExploration(status: ExplorationStatus): boolean {
  return !["completed", "cancelled", "interrupted", "failed"].includes(status);
}
