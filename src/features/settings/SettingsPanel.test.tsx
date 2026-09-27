import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SettingsPanel } from "./SettingsPanel";
import type {
  ShortcutBinding,
  ShortcutBindingView,
} from "../../lib/tauri/shortcut-client";

const { getShortcutBindingsMock, updateShortcutBindingsMock } = vi.hoisted(() => ({
  getShortcutBindingsMock: vi.fn<() => Promise<readonly ShortcutBindingView[]>>(),
  updateShortcutBindingsMock:
    vi.fn<
      (bindings: readonly ShortcutBinding[]) => Promise<readonly ShortcutBindingView[]>
    >(),
}));

vi.mock("../../lib/tauri/shortcut-client", async () => {
  const actual = await vi.importActual<
    typeof import("../../lib/tauri/shortcut-client")
  >("../../lib/tauri/shortcut-client");
  return {
    ...actual,
    getShortcutBindings: getShortcutBindingsMock,
    updateShortcutBindings: updateShortcutBindingsMock,
  };
});

const initialView: ShortcutBindingView = {
  binding: {
    id: "00000000-0000-4000-8000-000000000001",
    action: "screenshot" as const,
    chord: { modifiers: ["control", "super"] as const, key: "KeyP" },
  },
  registration: {
    status: "registered" as const,
    effectiveTrigger: "Ctrl+Super+P",
  },
};

describe("Binds settings", () => {
  beforeEach(() => {
    getShortcutBindingsMock.mockReset().mockResolvedValue([initialView]);
    updateShortcutBindingsMock.mockReset().mockImplementation((bindings) =>
      Promise.resolve(
        bindings.map((binding) => ({
          binding,
          registration:
            binding.chord === null
              ? { status: "unbound" as const }
              : { status: "registered" as const, effectiveTrigger: "Ctrl+Super+P" },
        })),
      ),
    );
  });

  it("adds and removes arbitrary rows while retaining one", async () => {
    render(<SettingsPanel onBack={() => {}} />);

    expect(await screen.findAllByTestId("shortcut-binding-row")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Add binding" }));
    expect(screen.getAllByTestId("shortcut-binding-row")).toHaveLength(2);

    const removeButtons = screen.getAllByRole("button", { name: "Remove binding" });
    const firstRemoveButton = removeButtons.at(0);
    expect(firstRemoveButton).toBeDefined();
    if (firstRemoveButton === undefined) {
      throw new Error("The first binding remove control is missing");
    }
    fireEvent.click(firstRemoveButton);
    expect(screen.getAllByTestId("shortcut-binding-row")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Remove binding" })).toBeDisabled();
  });

  it("records only while armed and Escape clears the row", async () => {
    render(<SettingsPanel onBack={() => {}} />);

    const row = await screen.findByTestId("shortcut-binding-row");
    const recordButton = within(row).getByRole("button", { name: "Record shortcut" });
    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "KeyQ", ctrlKey: true });
    expect(recordButton).toHaveTextContent("Ctrl + Q");

    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "Escape" });
    expect(recordButton).toHaveTextContent("Not set");
  });

  it("explains when the desktop session does not support global shortcuts", async () => {
    updateShortcutBindingsMock.mockRejectedValue({ code: "registrarUnavailable" });
    render(<SettingsPanel onBack={() => {}} />);

    const row = await screen.findByTestId("shortcut-binding-row");
    const recordButton = within(row).getByRole("button", { name: "Record shortcut" });
    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "KeyQ", ctrlKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Save bindings" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Global shortcut registration is unavailable in this desktop session",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Try an X11 session or a Wayland desktop with GlobalShortcuts support",
    );
  });
});
