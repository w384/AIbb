const DRAG_THRESHOLD = 4;

export interface PetState {
  status: "idle" | "chatting" | "exploring" | "returned" | "error";
  taskId: string | null;
}

export type PetEvent =
  | { type: "CHAT_OPENED" }
  | { type: "EXPLORATION_STARTED"; taskId: string }
  | { type: "EXPLORATION_COMPLETED"; taskId: string }
  | { type: "EXPLORATION_FAILED"; taskId: string }
  | { type: "RESET_TO_IDLE" };

export const initialPetState: PetState = { status: "idle", taskId: null };

export function petStatusReducer(state: PetState, event: PetEvent): PetState {
  switch (event.type) {
    case "CHAT_OPENED":
      return { status: "chatting", taskId: null };
    case "EXPLORATION_STARTED":
      return { status: "exploring", taskId: event.taskId };
    case "EXPLORATION_COMPLETED":
      return state.taskId === event.taskId
        ? { status: "returned", taskId: event.taskId }
        : state;
    case "EXPLORATION_FAILED":
      return state.taskId === event.taskId
        ? { status: "error", taskId: event.taskId }
        : state;
    case "RESET_TO_IDLE":
      return initialPetState;
  }
}

export interface PetGestureState {
  pointerId: number | null;
  origin: { x: number; y: number } | null;
  dragging: boolean;
  suppressClick: boolean;
}

export type PetGestureAction =
  | { type: "pointerDown"; pointerId: number; x: number; y: number }
  | {
      type: "pointerMove";
      pointerId: number;
      x: number;
      y: number;
      leftButtonPressed: boolean;
    }
  | { type: "pointerEnd"; pointerId: number }
  | { type: "clickConsumed" };

export const initialPetGestureState: PetGestureState = {
  pointerId: null,
  origin: null,
  dragging: false,
  suppressClick: false,
};

export function petReducer(
  state: PetGestureState,
  action: PetGestureAction,
): PetGestureState {
  switch (action.type) {
    case "pointerDown":
      return {
        pointerId: action.pointerId,
        origin: { x: action.x, y: action.y },
        dragging: false,
        suppressClick: false,
      };
    case "pointerMove": {
      if (state.pointerId !== action.pointerId || !state.origin || state.dragging) {
        return state;
      }
      if (!action.leftButtonPressed) {
        return { ...state, pointerId: null, origin: null, dragging: false };
      }
      const movement = Math.hypot(
        action.x - state.origin.x,
        action.y - state.origin.y,
      );
      if (movement <= DRAG_THRESHOLD) {
        return state;
      }
      return { ...state, dragging: true, suppressClick: true };
    }
    case "pointerEnd":
      if (state.pointerId !== action.pointerId) {
        return state;
      }
      return { ...state, pointerId: null, origin: null, dragging: false };
    case "clickConsumed":
      return { ...state, suppressClick: false };
  }
}
