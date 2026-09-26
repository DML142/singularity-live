import { create } from "zustand";

import type {
  HotkeyCaptureErrorKind,
  HotkeyCaptureEvent,
} from "../lib/tauri/hotkey-capture-client";
import {
  cancelScreenCapture,
  cropScreenCapture,
  discardScreenCapture,
  getScreenCaptureCapabilities,
  listScreenCaptureTargets,
  newCaptureOperationId,
  screenCaptureErrorMessage,
  startScreenCapture,
  type CapturePreview,
  type CaptureTargetKind,
  type CropRect,
  type ScreenCaptureCapabilities,
  type ScreenCaptureTarget,
} from "../lib/tauri/screen-assistance-client";
import { useManualAssistanceStore } from "./manual-assistance-store";

export type ScreenAssistancePhase =
  | "loading"
  | "ready"
  | "loadingTargets"
  | "selecting"
  | "capturing"
  | "preview"
  | "cropping"
  | "sending"
  | "sent"
  | "discarded"
  | "expired"
  | "cancelled"
  | "failed"
  | "unavailable";

interface ScreenAssistanceState {
  readonly phase: ScreenAssistancePhase;
  readonly capabilities: ScreenCaptureCapabilities | null;
  readonly targetKind: CaptureTargetKind;
  readonly targets: readonly ScreenCaptureTarget[];
  readonly selectedTargetId: string;
  readonly operationId: string | null;
  readonly preview: CapturePreview | null;
  readonly error: string | null;
  readonly loadCapabilities: () => Promise<void>;
  readonly loadTargets: (kind: CaptureTargetKind) => Promise<void>;
  readonly selectTarget: (targetId: string) => void;
  readonly capture: () => Promise<void>;
  readonly cancelCapture: () => Promise<void>;
  readonly crop: (rect: CropRect) => Promise<void>;
  readonly discard: () => Promise<void>;
  readonly send: () => Promise<void>;
  readonly clearForSessionReset: () => void;
  readonly clearOnUnmount: () => Promise<void>;
  readonly acceptHotkeyCapture: (event: HotkeyCaptureEvent) => void;
}

let expiryTimer: ReturnType<typeof setTimeout> | undefined;

function stopExpiryTimer(): void {
  if (expiryTimer !== undefined) {
    clearTimeout(expiryTimer);
    expiryTimer = undefined;
  }
}

function commandCode(error: unknown): string | null {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string"
  ) {
    return error.code;
  }
  return null;
}

export const useScreenAssistanceStore = create<ScreenAssistanceState>((set, get) => ({
  phase: "loading",
  capabilities: null,
  targetKind: "monitor",
  targets: [],
  selectedTargetId: "",
  operationId: null,
  preview: null,
  error: null,
  loadCapabilities: async () => {
    set({ phase: "loading", error: null });
    try {
      const capabilities = await getScreenCaptureCapabilities();
      const targetKind = capabilities.targets.includes("monitor")
        ? "monitor"
        : "window";
      set({ phase: "ready", capabilities, targetKind });
    } catch {
      set({
        phase: "unavailable",
        capabilities: null,
        error: "Screen capture capabilities are unavailable",
      });
    }
  },
  loadTargets: async (kind) => {
    const state = get();
    if (state.phase === "capturing" || state.phase === "sending") {
      return;
    }
    if (state.capabilities === null || !state.capabilities.targets.includes(kind)) {
      set({
        phase: "failed",
        targetKind: kind,
        targets: [],
        selectedTargetId: "",
        error: "This screen or window capture type is unavailable",
      });
      return;
    }
    set({
      phase: "loadingTargets",
      targetKind: kind,
      targets: [],
      selectedTargetId: "",
      error: null,
    });
    try {
      const targets = await listScreenCaptureTargets(kind);
      set({
        phase: "selecting",
        targets,
        selectedTargetId: targets[0]?.id ?? "",
      });
    } catch (error) {
      set({ phase: "failed", error: screenCaptureErrorMessage(error) });
    }
  },
  selectTarget: (selectedTargetId) => {
    set({ selectedTargetId, error: null });
  },
  capture: async () => {
    const state = get();
    if (
      state.phase === "capturing" ||
      state.phase === "sending" ||
      state.selectedTargetId.length === 0
    ) {
      return;
    }
    stopExpiryTimer();
    const operationId = newCaptureOperationId();
    set({
      phase: "capturing",
      operationId,
      preview: null,
      error: null,
      selectedTargetId: state.selectedTargetId,
    });
    try {
      const preview = await startScreenCapture(state.selectedTargetId, operationId);
      if (get().operationId !== operationId) {
        await discardScreenCapture(preview.captureId);
        return;
      }
      set({ phase: "preview", operationId: null, preview, error: null });
      expiryTimer = setTimeout(
        () => {
          if (get().preview?.captureId !== preview.captureId) {
            return;
          }
          set({
            phase: "expired",
            preview: null,
            error: "The screenshot expired. Capture it again to continue.",
          });
          void discardScreenCapture(preview.captureId).catch(() => {});
        },
        Math.max(0, preview.expiresInSeconds * 1000),
      );
    } catch (error) {
      if (get().operationId !== operationId) {
        return;
      }
      set({
        phase: commandCode(error) === "cancelled" ? "cancelled" : "failed",
        operationId: null,
        preview: null,
        error: screenCaptureErrorMessage(error),
      });
    }
  },
  cancelCapture: async () => {
    const operationId = get().operationId;
    if (operationId === null) {
      return;
    }
    try {
      await cancelScreenCapture(operationId);
    } catch (error) {
      set({ error: screenCaptureErrorMessage(error) });
    }
  },
  crop: async (rect) => {
    const preview = get().preview;
    if (preview === null || get().phase !== "preview") {
      return;
    }
    set({ phase: "cropping", error: null });
    try {
      const cropped = await cropScreenCapture(preview.captureId, rect);
      set({ phase: "preview", preview: cropped, error: null });
      stopExpiryTimer();
      expiryTimer = setTimeout(
        () => {
          if (get().preview?.captureId !== cropped.captureId) {
            return;
          }
          set({
            phase: "expired",
            preview: null,
            error: "The screenshot expired. Capture it again to continue.",
          });
          void discardScreenCapture(cropped.captureId).catch(() => {});
        },
        Math.max(0, cropped.expiresInSeconds * 1000),
      );
    } catch (error) {
      if (commandCode(error) === "imageExpired") {
        stopExpiryTimer();
        set({
          phase: "expired",
          preview: null,
          error: screenCaptureErrorMessage(error),
        });
      } else {
        set({ phase: "preview", error: screenCaptureErrorMessage(error) });
      }
    }
  },
  discard: async () => {
    const preview = get().preview;
    if (preview === null) {
      return;
    }
    stopExpiryTimer();
    set({ phase: "discarded", preview: null, operationId: null, error: null });
    try {
      await discardScreenCapture(preview.captureId);
    } catch (error) {
      if (commandCode(error) !== "imageExpired") {
        set({ error: screenCaptureErrorMessage(error) });
      }
    }
  },
  send: async () => {
    const preview = get().preview;
    if (preview === null || get().phase !== "preview") {
      return;
    }
    stopExpiryTimer();
    set({ phase: "sending", preview: null, error: null });
    const started = await useManualAssistanceStore
      .getState()
      .startScreenshot(preview.captureId);
    if (started) {
      set({ phase: "sent", error: null });
    } else {
      set({
        phase: "failed",
        error:
          useManualAssistanceStore.getState().error ??
          "The screenshot could not be sent. Try again.",
      });
    }
  },
  clearForSessionReset: () => {
    stopExpiryTimer();
    set({
      phase: "ready",
      targets: [],
      selectedTargetId: "",
      operationId: null,
      preview: null,
      error: null,
    });
  },
  clearOnUnmount: async () => {
    const preview = get().preview;
    const operationId = get().operationId;
    stopExpiryTimer();
    set({ preview: null, operationId: null, error: null });
    if (operationId !== null) {
      await cancelScreenCapture(operationId).catch(() => {});
    }
    if (preview !== null) {
      await discardScreenCapture(preview.captureId).catch(() => {});
    }
  },
  acceptHotkeyCapture: (event) => {
    if (event.status === "error" && event.kind === "busy") {
      set({ error: "A screenshot action is already in progress" });
      return;
    }
    stopExpiryTimer();
    if (event.status === "error") {
      set({
        phase: event.kind === "cancelled" ? "cancelled" : "failed",
        preview: null,
        operationId: null,
        error: hotkeyCaptureErrorMessage(event.kind),
      });
      return;
    }
    const preview = event.preview;
    set({ phase: "preview", operationId: null, preview, error: null });
    expiryTimer = setTimeout(
      () => {
        if (get().preview?.captureId !== preview.captureId) {
          return;
        }
        set({
          phase: "expired",
          preview: null,
          error: "The screenshot expired. Capture it again to continue.",
        });
        void discardScreenCapture(preview.captureId).catch(() => {});
      },
      Math.max(0, preview.expiresInSeconds * 1000),
    );
  },
}));

function hotkeyCaptureErrorMessage(kind: HotkeyCaptureErrorKind): string {
  const errorCodes: Record<HotkeyCaptureErrorKind, string> = {
    unsupported: "unsupported",
    permission_required: "permissionRequired",
    permission_denied: "permissionDenied",
    invalid_target: "invalidTarget",
    invalid_region: "invalidRegion",
    image_expired: "imageExpired",
    preparation: "preparation",
    cancelled: "cancelled",
    busy: "busy",
    no_matching_capture: "noMatchingCapture",
    unavailable: "unavailable",
  };
  return screenCaptureErrorMessage({ code: errorCodes[kind] });
}
