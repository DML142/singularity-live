import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import {
  formatShortcutChord,
  getShortcutBindings,
  updateShortcutBindings,
  type ShortcutAction,
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

  it("accepts all built-in screenshot, audio, window, and composer actions", () => {
    const actions: readonly ShortcutAction[] = [
      "screenshot_send",
      "screenshot_capture_send",
      "toggle_taskbar_icon",
      "toggle_audio_source",
      "toggle_click_through",
      "min_mode",
    ];
    expect(actions).toEqual([
      "screenshot_send",
      "screenshot_capture_send",
      "toggle_taskbar_icon",
      "toggle_audio_source",
      "toggle_click_through",
      "min_mode",
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
