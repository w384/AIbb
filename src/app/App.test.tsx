import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";

vi.mock("../features/pet/PetSurface", () => ({
  PetSurface: () => <main aria-label="pet-route">pet</main>,
}));
vi.mock("../features/chat/ChatPanel", () => ({
  ChatPanel: () => <main aria-label="chat-route">chat</main>,
}));
vi.mock("../features/settings/SettingsPanel", () => ({
  SettingsPanel: () => <main aria-label="settings-route">settings</main>,
}));

describe("App routing", () => {
  it.each([
    ["pet", "pet-route"],
    ["chat", "chat-route"],
    ["settings", "settings-route"],
  ])("routes the %s window label", (windowLabel, accessibleName) => {
    render(<App windowLabel={windowLabel} />);
    expect(screen.getByRole("main", { name: accessibleName })).toBeVisible();
  });
});
