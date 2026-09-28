import { create } from "zustand";

import {
  setVoiceInputSource,
  startVoiceInput,
  stopVoiceInput,
  subscribeVoiceInputEvents,
  type AudioInputSource,
  type VoiceInputEvent,
} from "../lib/tauri/voice-input-client";

type VoiceInputPhase = "idle" | "connecting" | "recording" | "stopping" | "failed";

interface FinalizedTranscript {
  readonly id: number;
  readonly text: string;
}

interface VoiceInputState {
  readonly phase: VoiceInputPhase;
  readonly source: AudioInputSource;
  readonly transcript: string;
  readonly error: string | null;
  readonly finalizedTranscript: FinalizedTranscript | null;
  readonly initialize: () => Promise<() => void>;
  readonly setSource: (source: AudioInputSource) => Promise<void>;
  readonly start: () => Promise<void>;
  readonly stop: () => Promise<void>;
  readonly handleEvent: (event: VoiceInputEvent) => void;
  readonly clearFinalizedTranscript: (id: number) => void;
}

let nextTranscriptId = 1;

function safeVoiceCommandError(error: unknown): string {
  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }
  return "Voice input could not start. Check the Soniox key and audio source.";
}

export const useVoiceInputStore = create<VoiceInputState>((set, get) => ({
  phase: "idle",
  source: "microphone",
  transcript: "",
  error: null,
  finalizedTranscript: null,
  initialize: async () => {
    try {
      return await subscribeVoiceInputEvents((event) => {
        get().handleEvent(event);
      });
    } catch {
      return () => {};
    }
  },
  setSource: async (source) => {
    if (get().phase === "recording" || get().phase === "connecting") {
      return;
    }
    set({ source, error: null });
    try {
      await setVoiceInputSource(source);
    } catch {
      set({ error: "The selected audio source could not be saved" });
    }
  },
  start: async () => {
    if (get().phase === "connecting" || get().phase === "recording") {
      return;
    }
    const source = get().source;
    set({ phase: "connecting", transcript: "", error: null });
    try {
      await startVoiceInput(source);
    } catch (error) {
      set({ phase: "failed", error: safeVoiceCommandError(error) });
    }
  },
  stop: async () => {
    if (get().phase !== "recording") {
      return;
    }
    set({ phase: "stopping", error: null });
    try {
      await stopVoiceInput();
    } catch {
      set({ phase: "failed", error: "Voice input could not be stopped cleanly" });
    }
  },
  handleEvent: (event) => {
    switch (event.type) {
      case "started":
        set({ phase: "recording", source: event.source, transcript: "", error: null });
        break;
      case "transcript":
        set({ transcript: event.text });
        break;
      case "stopped": {
        const transcript = event.transcript.trim();
        const finalizedTranscript =
          transcript.length > 0 ? { id: nextTranscriptId++, text: transcript } : null;
        set({ phase: "idle", transcript, error: null, finalizedTranscript });
        break;
      }
      case "failed":
        set({ phase: "failed", error: event.message });
        break;
    }
  },
  clearFinalizedTranscript: (id) => {
    if (get().finalizedTranscript?.id === id) {
      set({ finalizedTranscript: null });
    }
  },
}));
