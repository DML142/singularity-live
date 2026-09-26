import { beforeEach, describe, expect, it, vi } from "vitest";

const { listenMock } = vi.hoisted(() => ({
  listenMock:
    vi.fn<
      (
        event: string,
        callback: (event: { payload: unknown }) => void,
      ) => Promise<() => void>
    >(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  listenForHotkeyCapture,
  parseHotkeyCaptureEvent,
} from "./hotkey-capture-client";

const preview = {
  captureId: "00000000-0000-4000-8000-000000000001",
  dataUrl: "data:image/png;base64,AQID",
  width: 1,
  height: 1,
  expiresInSeconds: 300,
};

describe("hotkey capture event client", () => {
  beforeEach(() => listenMock.mockReset());

  it("accepts strict preview/error shapes and rejects image extras", () => {
    expect(parseHotkeyCaptureEvent({ status: "preview", preview })).toEqual({
      status: "preview",
      preview,
    });
    expect(
      parseHotkeyCaptureEvent({
        status: "error",
        kind: "permission_denied",
        message: "Screen capture permission was denied",
      }),
    ).toEqual({
      status: "error",
      kind: "permission_denied",
      message: "Screen capture permission was denied",
    });
    expect(
      parseHotkeyCaptureEvent({
        status: "preview",
        preview: { ...preview, apiKey: "secret" },
      }),
    ).toBeNull();
  });

  it("registers a listener and drops malformed events", async () => {
    let handler: ((event: { payload: unknown }) => void) | undefined;
    const unlisten = vi.fn();
    listenMock.mockImplementation((_event, callback) => {
      handler = callback;
      return Promise.resolve(unlisten);
    });
    const onCapture = vi.fn();

    const stop = await listenForHotkeyCapture(onCapture);
    handler?.({ payload: { status: "preview", preview } });
    handler?.({ payload: { status: "unknown" } });

    expect(listenMock).toHaveBeenCalledWith(
      "singularity:hotkey-capture",
      expect.any(Function),
    );
    expect(onCapture).toHaveBeenCalledTimes(1);
    stop();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
