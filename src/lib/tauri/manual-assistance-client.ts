import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const MAX_MANUAL_TEXT_BYTES = 16 * 1024;
const REQUEST_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export type ProviderId = "open_router";

export type ManualAssistanceReadiness =
  | {
      readonly status: "ready";
      readonly provider: ProviderId;
      readonly model: string;
      readonly contextPack: string;
    }
  | { readonly status: "unconfigured"; readonly message: string };

export interface Usage {
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly totalTokens: number;
}

export type ManualAssistanceErrorCode =
  | "authentication"
  | "configuration"
  | "invalidRequest"
  | "rateLimit"
  | "timeout"
  | "transport"
  | "provider"
  | "cancellation"
  | "malformedResponse";

export type ManualAssistanceEvent =
  | { readonly type: "started"; readonly requestId: string }
  | {
      readonly type: "textDelta";
      readonly requestId: string;
      readonly delta: string;
    }
  | {
      readonly type: "completed";
      readonly requestId: string;
      readonly provider: ProviderId;
      readonly model: string;
      readonly usage: Usage | null;
    }
  | { readonly type: "cancelled"; readonly requestId: string }
  | {
      readonly type: "failed";
      readonly requestId: string;
      readonly code: ManualAssistanceErrorCode;
      readonly message: string;
    };

export async function getManualAssistanceReadiness(): Promise<ManualAssistanceReadiness> {
  const response = await invoke<unknown>("get_manual_assistance_readiness");
  const readiness = parseReadiness(response);
  if (readiness === null) {
    throw new Error("Invalid manual assistance readiness response");
  }
  return readiness;
}

export async function startManualAssistance(text: string): Promise<string> {
  if (typeof text !== "string" || text.trim().length === 0) {
    throw new Error("Enter text before sending a request");
  }
  const normalizedText = text.trim();
  if (new TextEncoder().encode(normalizedText).byteLength > MAX_MANUAL_TEXT_BYTES) {
    throw new Error("Manual input exceeds the 16 KiB limit");
  }
  const response = await invoke<unknown>("start_manual_assistance", {
    request: { text: normalizedText },
  });
  if (
    !isRecord(response) ||
    !hasExactKeys(response, ["requestId"]) ||
    !isRequestId(response.requestId)
  ) {
    throw new Error("Invalid manual assistance start response");
  }
  return response.requestId;
}

export async function cancelManualAssistance(requestId: string): Promise<void> {
  if (!isRequestId(requestId)) {
    throw new Error("Invalid manual assistance request identifier");
  }
  await invoke("cancel_manual_assistance", {
    request: { requestId },
  });
}

export async function resetManualAssistanceSession(): Promise<void> {
  await invoke("reset_session");
}

export async function subscribeManualAssistanceEvents(
  onEvent: (event: ManualAssistanceEvent) => void,
): Promise<UnlistenFn> {
  return listen<unknown>("manual-assistance-event", ({ payload }) => {
    const event = parseEvent(payload);
    if (event !== null) {
      onEvent(event);
    }
  });
}

function parseReadiness(value: unknown): ManualAssistanceReadiness | null {
  if (!isRecord(value) || typeof value.status !== "string") {
    return null;
  }
  if (
    value.status === "ready" &&
    hasExactKeys(value, ["status", "provider", "model", "contextPack"]) &&
    value.provider === "open_router" &&
    isNonEmptyString(value.model) &&
    isNonEmptyString(value.contextPack)
  ) {
    return {
      status: "ready",
      provider: value.provider,
      model: value.model,
      contextPack: value.contextPack,
    };
  }
  if (
    value.status === "unconfigured" &&
    hasExactKeys(value, ["status", "message"]) &&
    isNonEmptyString(value.message)
  ) {
    return { status: "unconfigured", message: value.message };
  }
  return null;
}

function parseEvent(value: unknown): ManualAssistanceEvent | null {
  if (
    !isRecord(value) ||
    typeof value.type !== "string" ||
    !isRequestId(value.requestId)
  ) {
    return null;
  }

  switch (value.type) {
    case "started":
      return hasExactKeys(value, ["type", "requestId"])
        ? { type: "started", requestId: value.requestId }
        : null;
    case "textDelta":
      return hasExactKeys(value, ["type", "requestId", "delta"]) &&
        typeof value.delta === "string"
        ? {
            type: "textDelta",
            requestId: value.requestId,
            delta: value.delta,
          }
        : null;
    case "completed": {
      const usage = parseUsage(value.usage);
      return hasExactKeys(value, ["type", "requestId", "provider", "model", "usage"]) &&
        value.provider === "open_router" &&
        isNonEmptyString(value.model) &&
        (value.usage === null || usage !== null)
        ? {
            type: "completed",
            requestId: value.requestId,
            provider: value.provider,
            model: value.model,
            usage,
          }
        : null;
    }
    case "cancelled":
      return hasExactKeys(value, ["type", "requestId"])
        ? { type: "cancelled", requestId: value.requestId }
        : null;
    case "failed":
      if (
        !hasExactKeys(value, ["type", "requestId", "code", "message"]) ||
        typeof value.code !== "string" ||
        !isNonEmptyString(value.message)
      ) {
        return null;
      }
      return isErrorCode(value.code)
        ? {
            type: "failed",
            requestId: value.requestId,
            code: value.code,
            message: value.message,
          }
        : {
            type: "failed",
            requestId: value.requestId,
            code: "provider",
            message: "The provider request failed",
          };
    default:
      return null;
  }
}

function parseUsage(value: unknown): Usage | null {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["inputTokens", "outputTokens", "totalTokens"]) ||
    !isNonNegativeInteger(value.inputTokens) ||
    !isNonNegativeInteger(value.outputTokens) ||
    !isNonNegativeInteger(value.totalTokens)
  ) {
    return null;
  }
  return {
    inputTokens: value.inputTokens,
    outputTokens: value.outputTokens,
    totalTokens: value.totalTokens,
  };
}

function isErrorCode(value: string): value is ManualAssistanceErrorCode {
  return (
    value === "authentication" ||
    value === "configuration" ||
    value === "invalidRequest" ||
    value === "rateLimit" ||
    value === "timeout" ||
    value === "transport" ||
    value === "provider" ||
    value === "cancellation" ||
    value === "malformedResponse"
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
): boolean {
  const keys = Object.keys(value);
  return keys.length === expected.length && expected.every((key) => key in value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isRequestId(value: unknown): value is string {
  return typeof value === "string" && REQUEST_ID_PATTERN.test(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}
