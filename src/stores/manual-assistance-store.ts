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

type TurnPhase = Exclude<RequestPhase, "idle">;

interface ManualAssistanceTurn {
  readonly id: number;
  readonly prompt: string;
  readonly answer: string;
  readonly phase: TurnPhase;
  readonly error: string | null;
}

interface ManualAssistanceState {
  readonly readiness: ReadinessState;
  readonly phase: RequestPhase;
  readonly requestId: string | null;
  readonly answer: string;
  readonly error: string | null;
  readonly cancelPending: boolean;
  readonly turns: readonly ManualAssistanceTurn[];
  readonly activeTurnId: number | null;
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
let nextTurnId = 1;

function updateTurn(
  turns: readonly ManualAssistanceTurn[],
  turnId: number | null,
  update: (turn: ManualAssistanceTurn) => ManualAssistanceTurn,
): readonly ManualAssistanceTurn[] {
  if (turnId === null) {
    return turns;
  }
  return turns.map((turn) => (turn.id === turnId ? update(turn) : turn));
}

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
          turns: updateTurn(state.turns, state.activeTurnId, (turn) => ({
            ...turn,
            phase: "streaming",
            answer: "",
            error: null,
          })),
        };
      }
      return state.requestId === event.requestId && state.phase === "streaming"
        ? {}
        : null;
    case "textDelta":
      if (state.phase !== "streaming" || state.requestId !== event.requestId) {
        return null;
      }
      return {
        answer: state.answer + event.delta,
        turns: updateTurn(state.turns, state.activeTurnId, (turn) => ({
          ...turn,
          answer: turn.answer + event.delta,
        })),
      };
    case "completed":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "completed",
            cancelPending: false,
            error: null,
            turns: updateTurn(state.turns, state.activeTurnId, (turn) => ({
              ...turn,
              phase: "completed",
              error: null,
            })),
          }
        : null;
    case "cancelled":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "cancelled",
            cancelPending: false,
            error: null,
            turns: updateTurn(state.turns, state.activeTurnId, (turn) => ({
              ...turn,
              phase: "cancelled",
              error: null,
            })),
          }
        : null;
    case "failed":
      return state.phase === "streaming" && state.requestId === event.requestId
        ? {
            phase: "failed",
            cancelPending: false,
            error: event.message,
            turns: updateTurn(state.turns, state.activeTurnId, (turn) => ({
              ...turn,
              phase: "failed",
              error: event.message,
            })),
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
  turns: [],
  activeTurnId: null,
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
    const turnId = nextTurnId;
    nextTurnId += 1;
    set({
      phase: "starting",
      requestId: null,
      answer: "",
      error: null,
      cancelPending: false,
      activeTurnId: turnId,
      turns: [
        ...get().turns,
        {
          id: turnId,
          prompt: text,
          answer: "",
          phase: "starting",
          error: null,
        },
      ],
    });
    try {
      const requestId = await startManualAssistance(text);
      const state = get();
      if (state.phase === "starting") {
        set({
          phase: "streaming",
          requestId,
          turns: updateTurn(state.turns, turnId, (turn) => ({
            ...turn,
            phase: "streaming",
          })),
        });
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
          turns: updateTurn(get().turns, turnId, (turn) => ({
            ...turn,
            phase: "failed",
            error: "The assistant stream could not be matched to this request",
          })),
        });
      }
    } catch (error) {
      bufferedEvents = [];
      const message = safeCommandError(error);
      set({
        phase: "failed",
        requestId: null,
        error: message,
        cancelPending: false,
        turns: updateTurn(get().turns, turnId, (turn) => ({
          ...turn,
          phase: "failed",
          error: message,
        })),
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
        const message = safeCommandError(error);
        const current = get();
        set({
          cancelPending: false,
          error: message,
          turns: updateTurn(current.turns, current.activeTurnId, (turn) => ({
            ...turn,
            error: message,
          })),
        });
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
