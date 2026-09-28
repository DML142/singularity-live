import { invoke } from "@tauri-apps/api/core";

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const MODIFIERS = ["control", "alt", "shift", "super"] as const;

export type ShortcutModifier = (typeof MODIFIERS)[number];
export type ShortcutAction = "screenshot" | "voice_input";

export interface ShortcutChord {
  readonly modifiers: readonly ShortcutModifier[];
  readonly key: string;
}

export interface ShortcutBinding {
  readonly id: string;
  readonly action: ShortcutAction;
  readonly chord: ShortcutChord | null;
}

export type ShortcutRegistrationState =
  | { readonly status: "registered"; readonly effectiveTrigger: string }
  | { readonly status: "unbound" }
  | { readonly status: "failed"; readonly message: string };

export interface ShortcutBindingView {
  readonly binding: ShortcutBinding;
  readonly registration: ShortcutRegistrationState;
}

export async function getShortcutBindings(): Promise<readonly ShortcutBindingView[]> {
  const response = await invoke<unknown>("get_shortcut_bindings");
  const bindings = parseBindingViews(response);
  if (bindings === null) {
    throw new Error("Invalid shortcut bindings response");
  }
  return bindings;
}

export async function updateShortcutBindings(
  bindings: readonly ShortcutBinding[],
): Promise<readonly ShortcutBindingView[]> {
  if (
    bindings.length === 0 ||
    bindings.some((binding) => !isShortcutBinding(binding))
  ) {
    throw new Error("Invalid shortcut bindings request");
  }
  const response = await invoke<unknown>("update_shortcut_bindings", {
    request: { bindings },
  });
  const updated = parseBindingViews(response);
  if (updated === null) {
    throw new Error("Invalid shortcut bindings response");
  }
  return updated;
}

export function newShortcutBinding(): ShortcutBinding {
  return { id: crypto.randomUUID(), action: "screenshot", chord: null };
}

export function formatShortcutChord(
  chord: ShortcutChord | null,
  platform: "windows" | "macos" | "other" = detectShortcutPlatform(),
): string {
  if (chord === null) {
    return "Not set";
  }
  const labels: Record<ShortcutModifier, string> = {
    control: "Ctrl",
    alt: "Alt",
    shift: "Shift",
    super: platform === "windows" ? "Win" : platform === "macos" ? "Command" : "Super",
  };
  return [
    ...chord.modifiers.map((modifier) => labels[modifier]),
    keyLabel(chord.key),
  ].join(" + ");
}

function detectShortcutPlatform(): "windows" | "macos" | "other" {
  const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
  if (/Windows/i.test(userAgent)) {
    return "windows";
  }
  if (/Macintosh|Mac OS X/i.test(userAgent)) {
    return "macos";
  }
  return "other";
}

export function shortcutKeyLabel(code: string): string {
  return keyLabel(code);
}

function parseBindingViews(value: unknown): readonly ShortcutBindingView[] | null {
  if (!Array.isArray(value) || value.length === 0) {
    return null;
  }
  const views = value.map(parseBindingView);
  if (views.some((view) => view === null)) {
    return null;
  }
  return views.filter((view): view is ShortcutBindingView => view !== null);
}

function parseBindingView(value: unknown): ShortcutBindingView | null {
  if (!isRecord(value) || !hasExactKeys(value, ["binding", "registration"])) {
    return null;
  }
  const binding = parseShortcutBinding(value.binding);
  const registration = parseRegistration(value.registration);
  if (binding === null || registration === null) {
    return null;
  }
  if (binding.chord === null && registration.status !== "unbound") {
    return null;
  }
  if (binding.chord !== null && registration.status === "unbound") {
    return null;
  }
  return { binding, registration };
}

function parseShortcutBinding(value: unknown): ShortcutBinding | null {
  return isShortcutBinding(value) ? value : null;
}

function isShortcutBinding(value: unknown): value is ShortcutBinding {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["id", "action", "chord"]) &&
    isUuid(value.id) &&
    (value.action === "screenshot" || value.action === "voice_input") &&
    (value.chord === null || isShortcutChord(value.chord))
  );
}

function isShortcutChord(value: unknown): value is ShortcutChord {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["modifiers", "key"]) &&
    Array.isArray(value.modifiers) &&
    value.modifiers.length <= 2 &&
    value.modifiers.every(isShortcutModifier) &&
    new Set(value.modifiers).size === value.modifiers.length &&
    isSupportedKey(value.key)
  );
}

function parseRegistration(value: unknown): ShortcutRegistrationState | null {
  if (!isRecord(value) || typeof value.status !== "string") {
    return null;
  }
  if (
    value.status === "registered" &&
    hasExactKeys(value, ["status", "effectiveTrigger"]) &&
    isNonEmptyString(value.effectiveTrigger)
  ) {
    return { status: "registered", effectiveTrigger: value.effectiveTrigger };
  }
  if (value.status === "unbound" && hasExactKeys(value, ["status"])) {
    return { status: "unbound" };
  }
  if (
    value.status === "failed" &&
    hasExactKeys(value, ["status", "message"]) &&
    isNonEmptyString(value.message)
  ) {
    return { status: "failed", message: value.message };
  }
  return null;
}

function isSupportedKey(value: unknown): value is string {
  if (typeof value !== "string" || value === "Escape") {
    return false;
  }
  return (
    /^Key[A-Z]$/.test(value) ||
    /^Digit[0-9]$/.test(value) ||
    /^F([1-9]|1[0-9]|2[0-4])$/.test(value) ||
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
    ].includes(value)
  );
}

function isShortcutModifier(value: unknown): value is ShortcutModifier {
  return typeof value === "string" && MODIFIERS.some((modifier) => modifier === value);
}

function keyLabel(code: string): string {
  if (code.startsWith("Key")) {
    return code.slice(3);
  }
  if (code.startsWith("Digit")) {
    return code.slice(5);
  }
  if (code.startsWith("Arrow")) {
    return code.slice(5);
  }
  return code;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(
  value: Record<string, unknown>,
  expectedKeys: readonly string[],
): boolean {
  const keys = Object.keys(value);
  return (
    keys.length === expectedKeys.length &&
    expectedKeys.every((key) => Object.hasOwn(value, key))
  );
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && UUID_PATTERN.test(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}
