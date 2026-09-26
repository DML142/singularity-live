import type { ShortcutChord, ShortcutModifier } from "../../lib/tauri/shortcut-client";

export type ShortcutRecordingResult =
  | { readonly type: "recorded"; readonly chord: ShortcutChord }
  | { readonly type: "clear" }
  | { readonly type: "ignore" }
  | { readonly type: "invalid"; readonly message: string };

const MODIFIER_CODES = new Set([
  "AltLeft",
  "AltRight",
  "ControlLeft",
  "ControlRight",
  "MetaLeft",
  "MetaRight",
  "ShiftLeft",
  "ShiftRight",
]);

export function recordedShortcutFromEvent(
  event: KeyboardEvent,
): ShortcutRecordingResult {
  if (event.repeat) {
    return { type: "ignore" };
  }
  if (event.code === "Escape") {
    return { type: "clear" };
  }
  if (MODIFIER_CODES.has(event.code)) {
    return { type: "ignore" };
  }
  if (!isSupportedShortcutKey(event.code)) {
    return { type: "invalid", message: "This key is not supported" };
  }
  const modifiers: ShortcutModifier[] = [];
  if (event.ctrlKey) {
    modifiers.push("control");
  }
  if (event.altKey) {
    modifiers.push("alt");
  }
  if (event.shiftKey) {
    modifiers.push("shift");
  }
  if (event.metaKey) {
    modifiers.push("super");
  }
  if (modifiers.length > 2) {
    return {
      type: "invalid",
      message: "Use at most three simultaneous keys",
    };
  }
  return {
    type: "recorded",
    chord: { modifiers, key: event.code },
  };
}

function isSupportedShortcutKey(code: string): boolean {
  if (code === "Escape") {
    return false;
  }
  return (
    /^Key[A-Z]$/.test(code) ||
    /^Digit[0-9]$/.test(code) ||
    /^F([1-9]|1[0-9]|2[0-4])$/.test(code) ||
    [
      "Backquote",
      "Backslash",
      "Backspace",
      "BracketLeft",
      "BracketRight",
      "Comma",
      "Delete",
      "End",
      "Enter",
      "Equal",
      "Home",
      "Insert",
      "Minus",
      "PageDown",
      "PageUp",
      "Period",
      "Quote",
      "Semicolon",
      "Slash",
      "Space",
      "Tab",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
    ].includes(code)
  );
}
