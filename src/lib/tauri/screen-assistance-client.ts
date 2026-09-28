import { invoke } from "@tauri-apps/api/core";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const PNG_DATA_URL_PATTERN = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;

export type CaptureTargetKind = "monitor" | "window";

export type CapturePermission = "not_required" | "user_prompt" | "unknown" | "denied";

export interface ScreenCaptureCapabilities {
  readonly targets: readonly CaptureTargetKind[];
  readonly permission: CapturePermission;
  readonly message: string | null;
}

export interface ScreenCaptureTarget {
  readonly id: string;
  readonly label: string;
  readonly kind: CaptureTargetKind;
}

export interface CapturePreview {
  readonly captureId: string;
  readonly dataUrl: string;
  readonly width: number;
  readonly height: number;
  readonly expiresInSeconds: number;
}

export interface CropRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

export async function getScreenCaptureCapabilities(): Promise<ScreenCaptureCapabilities> {
  const response = await invoke<unknown>("get_screen_capture_capabilities");
  const capabilities = parseCapabilities(response);
  if (capabilities === null) {
    throw new Error("Invalid screen capture capabilities response");
  }
  return capabilities;
}

export async function listScreenCaptureTargets(
  kind: CaptureTargetKind,
): Promise<readonly ScreenCaptureTarget[]> {
  const response = await invoke<unknown>("list_screen_capture_targets", {
    request: { kind },
  });
  if (!Array.isArray(response)) {
    throw new Error("Invalid screen capture targets response");
  }
  const targets = response.map(parseTarget);
  if (targets.some((target) => target === null || target.kind !== kind)) {
    throw new Error("Invalid screen capture targets response");
  }
  return targets.filter((target): target is ScreenCaptureTarget => target !== null);
}

export async function startScreenCapture(
  targetId: string,
  operationId: string,
): Promise<CapturePreview> {
  if (typeof targetId !== "string" || targetId.trim().length === 0) {
    throw new Error("Invalid screen capture target");
  }
  if (!isUuid(operationId)) {
    throw new Error("Invalid capture operation identifier");
  }
  const response = await invoke<unknown>("start_screen_capture", {
    request: { targetId, operationId },
  });
  const preview = parseCapturePreview(response);
  if (preview === null) {
    throw new Error("Invalid screenshot preview response");
  }
  return preview;
}

export async function cancelScreenCapture(operationId: string): Promise<void> {
  if (!isUuid(operationId)) {
    throw new Error("Invalid capture operation identifier");
  }
  await invoke("cancel_screen_capture", { request: { operationId } });
}

export async function cropScreenCapture(
  captureId: string,
  rect: CropRect,
): Promise<CapturePreview> {
  assertCaptureId(captureId);
  if (
    !isNonNegativeInteger(rect.x) ||
    !isNonNegativeInteger(rect.y) ||
    !isPositiveInteger(rect.width) ||
    !isPositiveInteger(rect.height)
  ) {
    throw new Error("Invalid screenshot region");
  }
  const response = await invoke<unknown>("crop_screen_capture", {
    request: { captureId, rect },
  });
  const preview = parseCapturePreview(response);
  if (preview === null || preview.captureId !== captureId) {
    throw new Error("Invalid screenshot preview response");
  }
  return preview;
}

export async function discardScreenCapture(captureId: string): Promise<void> {
  assertCaptureId(captureId);
  await invoke("discard_screen_capture", { request: { captureId } });
}

export async function startScreenshotAssistance(captureId: string): Promise<string> {
  assertCaptureId(captureId);
  const response = await invoke<unknown>("start_screenshot_assistance", {
    request: { captureId },
  });
  if (
    !isRecord(response) ||
    !hasExactKeys(response, ["requestId"]) ||
    !isUuid(response.requestId)
  ) {
    throw new Error("Invalid screenshot assistance response");
  }
  return response.requestId;
}

export function newCaptureOperationId(): string {
  return crypto.randomUUID();
}

export function screenCaptureErrorMessage(error: unknown): string {
  if (!isRecord(error) || typeof error.code !== "string") {
    return "The screenshot action could not be completed. Try again.";
  }
  switch (error.code) {
    case "unsupported":
      return "Screen capture is unavailable on this desktop";
    case "permissionRequired":
      return "Allow screen capture in the system prompt, then try again";
    case "permissionDenied":
      return "Screen capture permission was denied. Check system privacy settings.";
    case "invalidTarget":
      return "The selected screen or window is no longer available";
    case "invalidRegion":
      return "The selected region is outside the screenshot";
    case "imageExpired":
      return "The screenshot expired. Capture it again to continue.";
    case "preparation":
      return "The screenshot could not be prepared";
    case "cancelled":
      return "Screenshot capture cancelled";
    case "busy":
      return "A screenshot action is already in progress";
    case "noMatchingCapture":
      return "This screenshot action is no longer active";
    case "noPreviousRequest":
      return "Ask a text question before sending a screenshot";
    case "notConfigured":
      return "Configure a text provider and context pack before sending a request";
    case "contextUnavailable":
      return "The configured context pack could not be loaded";
    case "unavailable":
      return "The screen sharing service is unavailable";
    default:
      return "The screenshot action could not be completed. Try again.";
  }
}

function parseCapabilities(value: unknown): ScreenCaptureCapabilities | null {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["targets", "permission", "message"]) ||
    !Array.isArray(value.targets) ||
    !value.targets.every(isCaptureTargetKind) ||
    !isCapturePermission(value.permission) ||
    !(value.message === null || isNonEmptyString(value.message))
  ) {
    return null;
  }
  return {
    targets: [...new Set(value.targets)],
    permission: value.permission,
    message: value.message,
  };
}

function parseTarget(value: unknown): ScreenCaptureTarget | null {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["id", "label", "kind"]) ||
    !isNonEmptyString(value.id) ||
    !isNonEmptyString(value.label) ||
    !isCaptureTargetKind(value.kind)
  ) {
    return null;
  }
  return { id: value.id, label: value.label, kind: value.kind };
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

function assertCaptureId(value: string): void {
  if (!isUuid(value)) {
    throw new Error("Invalid capture identifier");
  }
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && UUID_PATTERN.test(value);
}

function isCaptureTargetKind(value: unknown): value is CaptureTargetKind {
  return value === "monitor" || value === "window";
}

function isCapturePermission(value: unknown): value is CapturePermission {
  return (
    value === "not_required" ||
    value === "user_prompt" ||
    value === "unknown" ||
    value === "denied"
  );
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isPositiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}
