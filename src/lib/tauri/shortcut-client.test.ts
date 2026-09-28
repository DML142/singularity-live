import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  formatShortcutChord,
  getShortcutBindings,
  updateShortcutBindings,
  type ShortcutBindingView,
} from "./shortcut-client";

const bindingView: ShortcutBindingView = {
  binding: {
    id: "00000000-0000-4000-8000-000000000001",
    action: "screenshot",
    chord: {
      modifiers: ["control", "super"],
      key: "KeyP",
    },
  },
  registration: { status: "registered", effectiveTrigger: "Ctrl+Super+P" },
};

describe("shortcut settings IPC client", () => {
  beforeEach(() => invokeMock.mockReset());

  it("loads only strictly validated binding views", async () => {
    invokeMock.mockResolvedValue([bindingView]);

    await expect(getShortcutBindings()).resolves.toEqual([bindingView]);
    expect(invokeMock).toHaveBeenCalledWith("get_shortcut_bindings");

    invokeMock.mockResolvedValue([{ ...bindingView, apiKey: "secret" }]);
    await expect(getShortcutBindings()).rejects.toThrow(
      "Invalid shortcut bindings response",
    );
  });

  it("updates non-secret rows and rejects unknown response fields", async () => {
    invokeMock.mockResolvedValue([bindingView]);

    await expect(updateShortcutBindings([bindingView.binding])).resolves.toEqual([
      bindingView,
    ]);
    expect(invokeMock).toHaveBeenCalledWith("update_shortcut_bindings", {
      request: { bindings: [bindingView.binding] },
    });

    invokeMock.mockResolvedValue([
      { ...bindingView, image: "data:image/png;base64,..." },
    ]);
    await expect(updateShortcutBindings([bindingView.binding])).rejects.toThrow(
      "Invalid shortcut bindings response",
    );
  });

  it("accepts quick-send bindings returned by Rust", async () => {
    const quickSendView: ShortcutBindingView = {
      ...bindingView,
      binding: { ...bindingView.binding, action: "quick_send", chord: null },
      registration: { status: "unbound" },
    };
    invokeMock.mockResolvedValue([quickSendView]);

    await expect(updateShortcutBindings([quickSendView.binding])).resolves.toEqual([
      quickSendView,
    ]);
  });

  it("displays the platform-equivalent Super key label", () => {
    const chord = bindingView.binding.chord;
    expect(chord).not.toBeNull();
    if (chord === null) {
      throw new Error("The test chord is required");
    }
    expect(formatShortcutChord(chord, "windows")).toBe("Ctrl + Win + P");
    expect(formatShortcutChord(chord, "macos")).toBe("Ctrl + Command + P");
    expect(formatShortcutChord(chord, "other")).toBe("Ctrl + Super + P");
  });
});
