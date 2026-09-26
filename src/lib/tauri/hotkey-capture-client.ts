import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { CapturePreview } from "./screen-assistance-client";

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const PNG_DATA_URL_PATTERN = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;
const ERROR_KINDS = [
  "unsupported",
  "permission_required",
  "permission_denied",
  "invalid_target",
  "invalid_region",
  "image_expired",
  "preparation",
  "cancelled",
  "busy",
  "no_matching_capture",
  "unavailable",
] as const;

export type HotkeyCaptureErrorKind = (typeof ERROR_KINDS)[number];

export type HotkeyCaptureEvent =
  | { readonly status: "preview"; readonly preview: CapturePreview }
  | {
      readonly status: "error";
      readonly kind: HotkeyCaptureErrorKind;
      readonly message: string;
    };

export async function listenForHotkeyCapture(
  onCapture: (event: HotkeyCaptureEvent) => void,
): Promise<UnlistenFn> {
  return listen<unknown>("singularity:hotkey-capture", (event) => {
    const parsed = parseHotkeyCaptureEvent(event.payload);
    if (parsed !== null) {
      onCapture(parsed);
    }
  });
}

export function parseHotkeyCaptureEvent(value: unknown): HotkeyCaptureEvent | null {
  if (!isRecord(value) || typeof value.status !== "string") {
    return null;
  }
  if (value.status === "preview" && hasExactKeys(value, ["status", "preview"])) {
    const preview = parseCapturePreview(value.preview);
    return preview === null ? null : { status: "preview", preview };
  }
  if (
    value.status === "error" &&
    hasExactKeys(value, ["status", "kind", "message"]) &&
    isHotkeyCaptureErrorKind(value.kind) &&
    isNonEmptyString(value.message)
  ) {
    return { status: "error", kind: value.kind, message: value.message };
  }
  return null;
}

function parseCapturePreview(value: unknown): CapturePreview | null {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "captureId",
      "dataUrl",
      "width",
      "height",
      "expiresInSeconds",
    ]) ||
    !isUuid(value.captureId) ||
    typeof value.dataUrl !== "string" ||
    !PNG_DATA_URL_PATTERN.test(value.dataUrl) ||
    !isPositiveInteger(value.width) ||
    !isPositiveInteger(value.height) ||
    !isNonNegativeInteger(value.expiresInSeconds)
  ) {
    return null;
  }
  return {
    captureId: value.captureId,
    dataUrl: value.dataUrl,
    width: value.width,
    height: value.height,
    expiresInSeconds: value.expiresInSeconds,
  };
}

function isHotkeyCaptureErrorKind(value: unknown): value is HotkeyCaptureErrorKind {
  return typeof value === "string" && ERROR_KINDS.some((kind) => kind === value);
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

function isPositiveInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) > 0;
}

function isNonNegativeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}
