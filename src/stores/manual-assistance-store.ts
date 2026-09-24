import { create } from "zustand";

import {
  cancelManualAssistance,
  getManualAssistanceReadiness,
  startManualAssistance,
  subscribeManualAssistanceEvents,
  type ManualAssistanceEvent,
  type ManualAssistanceReadiness,
} from "../lib/tauri/manual-assistance-client";

type ReadinessState =
  | { readonly phase: "loading" }
  | {
      readonly phase: "ready";
      readonly value: Extract<ManualAssistanceReadiness, { status: "ready" }>;
    }
  | { readonly phase: "unconfigured"; readonly message: string }
  | { readonly phase: "unavailable" };

type RequestPhase =
  "idle" | "starting" | "streaming" | "completed" | "cancelled" | "failed";

interface ManualAssistanceState {
  readonly readiness: ReadinessState;
  readonly phase: RequestPhase;
  readonly requestId: string | null;
  readonly answer: string;
  readonly error: string | null;
  readonly cancelPending: boolean;
  readonly loadReadiness: () => Promise<void>;
  readonly initialize: () => Promise<() => void>;
  readonly start: (text: string) => Promise<void>;
  readonly cancel: () => Promise<void>;
  readonly handleEvent: (event: ManualAssistanceEvent) => void;
}

const MAX_BUFFERED_EVENTS = 128;
const RETIRED_REQUEST_LIMIT = 32;
const retiredRequestIds = new Set<string>();
let bufferedEvents: ManualAssistanceEvent[] = [];

function retireRequest(requestId: string): void {
  retiredRequestIds.add(requestId);
  if (retiredRequestIds.size > RETIRED_REQUEST_LIMIT) {
    const oldest = retiredRequestIds.values().next().value;
    if (oldest !== undefined) {
      retiredRequestIds.delete(oldest);
    }
  }
}

function safeCommandError(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error) {
    switch (error.code) {
      case "emptyInput":
        return "Enter text before sending a request";
      case "inputTooLarge":
        return "Keep your request under 16 KiB";
      case "busy":
        return "A request is already in progress";
      case "notConfigured":
        return "Configure OpenRouter and a context pack before sending a request";
      case "contextUnavailable":
        return "The configured context pack could not be loaded";
      case "eventConsumerUnavailable":
        return "The assistant stream is unavailable. Try again.";
      case "invalidRequestId":
      case "noMatchingRequest":
        return "This request is no longer active";
      default:
        return "The request could not be completed. Try again.";
    }
  }
  return "The request could not be completed. Try again.";
}

function applyEvent(
  state: ManualAssistanceState,
  event: ManualAssistanceEvent,
): Partial<ManualAssistanceState> | null {
  switch (event.type) {
    case "started":
      if (state.phase === "starting") {
        return {
          phase: "streaming",
          requestId: event.requestId,
          answer: "",
          error: null,
        };
      }
      return state.requestId === event.requestId && state.phase === "streaming"
        ? {}
        : null;
    case "textDelta":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? { answer: state.answer + event.delta }
        : null;
    case "completed":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "completed",
            cancelPending: false,
            error: null,
          }
        : null;
    case "cancelled":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "cancelled",
            cancelPending: false,
            error: null,
          }
        : null;
    case "failed":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "failed",
            cancelPending: false,
            error: event.message,
          }
        : null;
  }
}

export const useManualAssistanceStore = create<ManualAssistanceState>((set, get) => ({
  readiness: { phase: "loading" },
  phase: "idle",
  requestId: null,
  answer: "",
  error: null,
  cancelPending: false,
  loadReadiness: async () => {
    set({ readiness: { phase: "loading" } });
    try {
      const readiness = await getManualAssistanceReadiness();
      set(
        readiness.status === "ready"
          ? { readiness: { phase: "ready", value: readiness } }
          : {
              readiness: {
                phase: "unconfigured",
                message: readiness.message,
              },
            },
      );
    } catch {
      set({ readiness: { phase: "unavailable" } });
    }
  },
  initialize: async () => {
    try {
      const unlisten = await subscribeManualAssistanceEvents((event) => {
        get().handleEvent(event);
      });
      void get().loadReadiness();
      return unlisten;
    } catch {
      set({ readiness: { phase: "unavailable" } });
      return () => {};
    }
  },
  start: async (text) => {
    if (get().phase === "starting" || get().phase === "streaming") {
      return;
    }
    bufferedEvents = [];
    set({
      phase: "starting",
      requestId: null,
      answer: "",
      error: null,
      cancelPending: false,
    });
    try {
      const requestId = await startManualAssistance(text);
      const state = get();
      if (state.phase === "starting") {
        set({ phase: "streaming", requestId });
      }
      const queued = bufferedEvents;
      bufferedEvents = [];
      for (const event of queued) {
        if (event.requestId === requestId) {
          get().handleEvent(event);
        }
      }
      if (get().requestId !== requestId && get().phase === "streaming") {
        retireRequest(requestId);
        set({
          phase: "failed",
          requestId,
          error: "The assistant stream could not be matched to this request",
        });
      }
    } catch (error) {
      bufferedEvents = [];
      set({
        phase: "failed",
        requestId: null,
        error: safeCommandError(error),
        cancelPending: false,
      });
    }
  },
  cancel: async () => {
    const state = get();
    if (
      state.phase !== "streaming" ||
      state.requestId === null ||
      state.cancelPending
    ) {
      return;
    }
    const requestId = state.requestId;
    set({ cancelPending: true, error: null });
    try {
      await cancelManualAssistance(requestId);
    } catch (error) {
      if (get().requestId === requestId && get().phase === "streaming") {
        set({ cancelPending: false, error: safeCommandError(error) });
      }
    }
  },
  handleEvent: (event) => {
    if (retiredRequestIds.has(event.requestId)) {
      return;
    }
    const state = get();
    if (state.phase === "starting" && state.requestId === null) {
      if (bufferedEvents.length < MAX_BUFFERED_EVENTS) {
        bufferedEvents = [...bufferedEvents, event];
      }
      return;
    }
    const change = applyEvent(state, event);
    if (change === null) {
      return;
    }
    if (
      event.type === "completed" ||
      event.type === "cancelled" ||
      event.type === "failed"
    ) {
      retireRequest(event.requestId);
    }
    set(change);
  },
}));
