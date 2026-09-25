import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  cancelManualAssistance,
  getManualAssistanceReadiness,
  resetManualAssistanceSession,
  startManualAssistance,
  subscribeManualAssistanceEvents,
} from "./manual-assistance-client";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);
const requestId = "00000000-0000-4000-8000-000000000001";

describe("manual assistance IPC client", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
  });

  it("validates readiness and start responses at runtime", async () => {
    invokeMock.mockResolvedValueOnce({
      status: "ready",
      provider: "open_router",
      model: "openrouter/free",
      contextPack: "fictional",
    });
    invokeMock.mockResolvedValueOnce({ requestId });

    await expect(getManualAssistanceReadiness()).resolves.toEqual({
      status: "ready",
      provider: "open_router",
      model: "openrouter/free",
      contextPack: "fictional",
    });
    await expect(startManualAssistance("Draft a reply")).resolves.toBe(requestId);
    expect(invokeMock).toHaveBeenLastCalledWith("start_manual_assistance", {
      request: { text: "Draft a reply" },
    });

    invokeMock.mockResolvedValueOnce({ status: "ready", apiKey: "leak" });
    await expect(getManualAssistanceReadiness()).rejects.toThrow(
      "Invalid manual assistance readiness response",
    );
  });

  it("validates outgoing request text and cancellation identifiers", async () => {
    await expect(startManualAssistance(" \n ")).rejects.toThrow(
      "Enter text before sending a request",
    );
    await expect(startManualAssistance("x".repeat(16 * 1024 + 1))).rejects.toThrow(
      "Manual input exceeds the 16 KiB limit",
    );
    await expect(startManualAssistance("🙂".repeat(4097))).rejects.toThrow(
      "Manual input exceeds the 16 KiB limit",
    );
    await expect(cancelManualAssistance("not-an-id")).rejects.toThrow(
      "Invalid manual assistance request identifier",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("invokes the Rust session reset command", async () => {
    await resetManualAssistanceSession();

    expect(invokeMock).toHaveBeenCalledWith("reset_session");
  });

  it("delivers valid events, ignores malformed events, and normalizes unknown failures", async () => {
    let receive: EventCallback<unknown> | undefined;
    listenMock.mockImplementation((_eventName, handler) => {
      receive = handler;
      return Promise.resolve(vi.fn());
    });
    const onEvent = vi.fn();

    await subscribeManualAssistanceEvents(onEvent);
    expect(listenMock).toHaveBeenCalledWith(
      "manual-assistance-event",
      expect.any(Function),
    );

    receive?.({
      event: "manual-assistance-event",
      id: 1,
      payload: { type: "textDelta", requestId, delta: "Hello" },
    });
    receive?.({
      event: "manual-assistance-event",
      id: 2,
      payload: { type: "textDelta", requestId: 7, delta: "bad" },
    });
    receive?.({
      event: "manual-assistance-event",
      id: 3,
      payload: {
        type: "failed",
        requestId,
        code: "newProviderError",
        message: "Untrusted detail",
      },
    });

    expect(onEvent).toHaveBeenNthCalledWith(1, {
      type: "textDelta",
      requestId,
      delta: "Hello",
    });
    expect(onEvent).toHaveBeenNthCalledWith(2, {
      type: "failed",
      requestId,
      code: "provider",
      message: "The provider request failed",
    });
    expect(onEvent).toHaveBeenCalledTimes(2);
  });
});
