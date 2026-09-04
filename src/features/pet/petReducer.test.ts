import { describe, expect, it } from "vitest";
import { initialPetState, petStatusReducer } from "./petReducer";

describe("petStatusReducer", () => {
  it("moves returned to idle only after the completed result is opened", () => {
    const exploring = petStatusReducer(initialPetState, {
      type: "EXPLORATION_STARTED",
      taskId: "task-1",
    });
    const returned = petStatusReducer(exploring, {
      type: "EXPLORATION_COMPLETED",
      taskId: "task-1",
    });
    expect(returned).toEqual({ status: "returned", taskId: "task-1" });

    expect(petStatusReducer(returned, { type: "RESET_TO_IDLE" })).toEqual(
      initialPetState,
    );
  });

  it("ignores terminal events for another exploration and exposes failures", () => {
    const exploring = petStatusReducer(initialPetState, {
      type: "EXPLORATION_STARTED",
      taskId: "task-1",
    });
    expect(
      petStatusReducer(exploring, {
        type: "EXPLORATION_COMPLETED",
        taskId: "other",
      }),
    ).toBe(exploring);
    expect(
      petStatusReducer(exploring, {
        type: "EXPLORATION_FAILED",
        taskId: "task-1",
      }),
    ).toEqual({ status: "error", taskId: "task-1" });
  });

  it("does not let late progress from an older task replace the active outing", () => {
    const active = petStatusReducer(initialPetState, {
      type: "EXPLORATION_STARTED",
      taskId: "task-current",
    });
    const afterLateProgress = petStatusReducer(active, {
      type: "EXPLORATION_STARTED",
      taskId: "task-older",
    });

    expect(afterLateProgress).toBe(active);
    expect(
      petStatusReducer(afterLateProgress, {
        type: "EXPLORATION_COMPLETED",
        taskId: "task-older",
      }),
    ).toBe(active);
  });
});
