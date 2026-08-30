const DRAG_THRESHOLD = 4;

export interface PetGestureState {
  pointerId: number | null;
  origin: { x: number; y: number } | null;
  dragging: boolean;
  suppressClick: boolean;
}

export type PetGestureAction =
  | { type: "pointerDown"; pointerId: number; x: number; y: number }
  | { type: "pointerMove"; pointerId: number; x: number; y: number }
  | { type: "pointerUp"; pointerId: number }
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
      const movement = Math.hypot(
        action.x - state.origin.x,
        action.y - state.origin.y,
      );
      if (movement <= DRAG_THRESHOLD) {
        return state;
      }
      return { ...state, dragging: true, suppressClick: true };
    }
    case "pointerUp":
      if (state.pointerId !== action.pointerId) {
        return state;
      }
      return { ...state, pointerId: null, origin: null, dragging: false };
    case "clickConsumed":
      return { ...state, suppressClick: false };
  }
}
