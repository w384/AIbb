import { describe, expect, it } from "vitest";
import type { BootstrapState } from "./contracts";

describe("BootstrapState", () => {
  it("accepts the Rust camelCase payload", () => {
    const state: BootstrapState = {
      firstRun: true,
      petStatus: "idle",
      apiConfigured: false,
    };
    expect(state.firstRun).toBe(true);
  });
});
