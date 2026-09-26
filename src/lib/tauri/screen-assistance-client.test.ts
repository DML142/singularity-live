import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  cancelScreenCapture,
  cropScreenCapture,
  discardScreenCapture,
  getScreenCaptureCapabilities,
  listScreenCaptureTargets,
  startScreenCapture,
  startScreenshotAssistance,
} from "./screen-assistance-client";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const operationId = "00000000-0000-4000-8000-000000000001";
const captureId = "00000000-0000-4000-8000-000000000002";
const preview = {
  captureId,
  dataUrl: "data:image/png;base64,AAEC",
  width: 2,
  height: 2,
  expiresInSeconds: 300,
};

describe("screen assistance IPC client", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("validates platform capability and target responses", async () => {
    invokeMock.mockResolvedValueOnce({
      targets: ["monitor", "window"],
      permission: "user_prompt",
      message: null,
    });
    invokeMock.mockResolvedValueOnce([
      { id: "monitor:1", label: "Display 1", kind: "monitor" },
    ]);

    await expect(getScreenCaptureCapabilities()).resolves.toEqual({
      targets: ["monitor", "window"],
      permission: "user_prompt",
      message: null,
    });
    await expect(listScreenCaptureTargets("monitor")).resolves.toEqual([
      { id: "monitor:1", label: "Display 1", kind: "monitor" },
    ]);
    expect(invokeMock).toHaveBeenLastCalledWith("list_screen_capture_targets", {
      request: { kind: "monitor" },
    });

    invokeMock.mockResolvedValueOnce({
      targets: ["monitor"],
      permission: "user_prompt",
      message: null,
      apiKey: "private",
    });
    await expect(getScreenCaptureCapabilities()).rejects.toThrow(
      "Invalid screen capture capabilities response",
    );
  });

  it("validates capture preview identifiers and region actions", async () => {
    invokeMock.mockResolvedValueOnce(preview);
    await expect(startScreenCapture("monitor:1", operationId)).resolves.toEqual(
      preview,
    );
    expect(invokeMock).toHaveBeenLastCalledWith("start_screen_capture", {
      request: { targetId: "monitor:1", operationId },
    });

    invokeMock.mockResolvedValueOnce(preview);
    await expect(
      cropScreenCapture(captureId, { x: 1, y: 2, width: 3, height: 4 }),
    ).resolves.toEqual(preview);
    expect(invokeMock).toHaveBeenLastCalledWith("crop_screen_capture", {
      request: { captureId, rect: { x: 1, y: 2, width: 3, height: 4 } },
    });

    invokeMock.mockResolvedValueOnce({
      ...preview,
      dataUrl: "data:image/png;base64, private",
    });
    await expect(startScreenCapture("monitor:1", operationId)).rejects.toThrow(
      "Invalid screenshot preview response",
    );
  });

  it("invokes cancel, discard, and explicit send commands with opaque identifiers", async () => {
    await cancelScreenCapture(operationId);
    await discardScreenCapture(captureId);
    invokeMock.mockResolvedValueOnce({ requestId: operationId });
    await expect(startScreenshotAssistance(captureId)).resolves.toBe(operationId);
    expect(invokeMock.mock.calls).toEqual([
      ["cancel_screen_capture", { request: { operationId } }],
      ["discard_screen_capture", { request: { captureId } }],
      ["start_screenshot_assistance", { request: { captureId } }],
    ]);
  });

  it("rejects malformed identifiers before invoking Rust", async () => {
    await expect(startScreenCapture("monitor:1", "invalid")).rejects.toThrow(
      "Invalid capture operation identifier",
    );
    await expect(cancelScreenCapture("invalid")).rejects.toThrow(
      "Invalid capture operation identifier",
    );
    await expect(discardScreenCapture("invalid")).rejects.toThrow(
      "Invalid capture identifier",
    );
    await expect(startScreenshotAssistance("invalid")).rejects.toThrow(
      "Invalid capture identifier",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
