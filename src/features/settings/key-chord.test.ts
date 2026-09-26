import { describe, expect, it } from "vitest";

import { recordedShortcutFromEvent } from "./key-chord";

describe("shortcut chord recording", () => {
  it("records one-, two-, and three-key simultaneous chords", () => {
    expect(
      recordedShortcutFromEvent(new KeyboardEvent("keydown", { code: "KeyP" })),
    ).toEqual({
      type: "recorded",
      chord: { modifiers: [], key: "KeyP" },
    });
    expect(
      recordedShortcutFromEvent(
        new KeyboardEvent("keydown", { code: "KeyP", ctrlKey: true }),
      ),
    ).toEqual({
      type: "recorded",
      chord: { modifiers: ["control"], key: "KeyP" },
    });
    expect(
      recordedShortcutFromEvent(
        new KeyboardEvent("keydown", {
          code: "KeyP",
          ctrlKey: true,
          metaKey: true,
        }),
      ),
    ).toEqual({
      type: "recorded",
      chord: { modifiers: ["control", "super"], key: "KeyP" },
    });
  });

  it("clears with Escape, ignores modifier-only presses, and rejects four simultaneous keys", () => {
    expect(
      recordedShortcutFromEvent(new KeyboardEvent("keydown", { code: "Escape" })),
    ).toEqual({ type: "clear" });
    expect(
      recordedShortcutFromEvent(
        new KeyboardEvent("keydown", { code: "ControlLeft", ctrlKey: true }),
      ),
    ).toEqual({ type: "ignore" });
    expect(
      recordedShortcutFromEvent(
        new KeyboardEvent("keydown", {
          code: "KeyP",
          ctrlKey: true,
          altKey: true,
          shiftKey: true,
        }),
      ),
    ).toEqual({ type: "invalid", message: "Use at most three simultaneous keys" });
  });
});
