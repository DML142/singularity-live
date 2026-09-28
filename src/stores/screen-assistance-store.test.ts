import { beforeEach, describe, expect, it, vi } from "vitest";

const { startScreenshotAssistanceMock, discardScreenCaptureMock } = vi.hoisted(() => ({
  startScreenshotAssistanceMock: vi.fn(),
  discardScreenCaptureMock: vi.fn(),
}));

vi.mock("../lib/tauri/screen-assistance-client", async () => {
  const actual = await vi.importActual<
    typeof import("../lib/tauri/screen-assistance-client")
  >("../lib/tauri/screen-assistance-client");
  return {
    ...actual,
    startScreenshotAssistance: startScreenshotAssistanceMock,
    discardScreenCapture: discardScreenCaptureMock,
  };
});

import { useScreenAssistanceStore } from "./screen-assistance-store";

const preview = {
  captureId: "00000000-0000-4000-8000-000000000001",
  dataUrl: "data:image/png;base64,AQID",
  width: 1,
  height: 1,
  expiresInSeconds: 300,
};

describe("screen assistance hotkey hydration", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    startScreenshotAssistanceMock.mockReset();
    discardScreenCaptureMock.mockReset().mockResolvedValue(undefined);
    useScreenAssistanceStore.setState({
      phase: "ready",
      preview: null,
      operationId: null,
      error: null,
    });
  });

  it("shows a hotkey preview in the existing explicit send/discard lifecycle", () => {
    useScreenAssistanceStore
      .getState()
      .acceptHotkeyCapture({ status: "preview", preview });

    expect(useScreenAssistanceStore.getState().phase).toBe("preview");
    expect(useScreenAssistanceStore.getState().preview).toEqual(preview);
    expect(startScreenshotAssistanceMock).not.toHaveBeenCalled();
  });

  it("displays a safe hotkey failure without retaining a stale preview", () => {
    useScreenAssistanceStore.setState({ phase: "preview", preview, operationId: null });

    useScreenAssistanceStore.getState().acceptHotkeyCapture({
      status: "error",
      kind: "permission_denied",
      message: "Screen capture permission was denied",
    });

    expect(useScreenAssistanceStore.getState().phase).toBe("failed");
    expect(useScreenAssistanceStore.getState().preview).toBeNull();
    expect(useScreenAssistanceStore.getState().error).toMatch(/permission/i);
  });
});
