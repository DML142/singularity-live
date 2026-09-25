import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ManualAssistanceEvent } from "../../lib/tauri/manual-assistance-client";
import { useManualAssistanceStore } from "../../stores/manual-assistance-store";
import { AssistantPanel } from "./AssistantPanel";

const client = vi.hoisted(() => ({
  getReadiness: vi.fn(),
  start: vi.fn(),
  cancel: vi.fn(),
  subscribe: vi.fn(),
}));

vi.mock("../../lib/tauri/manual-assistance-client", () => ({
  getManualAssistanceReadiness: client.getReadiness,
  startManualAssistance: client.start,
  cancelManualAssistance: client.cancel,
  subscribeManualAssistanceEvents: client.subscribe,
}));

let receiveEvent: ((event: ManualAssistanceEvent) => void) | undefined;
let requestNumber = 0;

describe("manual assistant panel", () => {
  beforeEach(() => {
    client.getReadiness.mockReset();
    client.start.mockReset();
    client.cancel.mockReset();
    client.subscribe.mockReset();
    receiveEvent = undefined;
    client.getReadiness.mockResolvedValue({
      status: "ready",
      provider: "open_router",
      model: "openrouter/free",
      contextPack: "fictional",
    });
    client.start.mockImplementation(() => {
      requestNumber += 1;
      return Promise.resolve(`request-${String(requestNumber)}`);
    });
    client.cancel.mockResolvedValue(undefined);
    client.subscribe.mockImplementation(
      (listener: (event: ManualAssistanceEvent) => void) => {
        receiveEvent = listener;
        return Promise.resolve(vi.fn());
      },
    );
    useManualAssistanceStore.setState({
      readiness: { phase: "loading" },
      phase: "idle",
      requestId: null,
      answer: "",
      error: null,
      cancelPending: false,
      turns: [],
      activeTurnId: null,
    });
  });

  it("renders streamed text, ignores stale events, and restores focus on completion", async () => {
    let resolveStart: ((requestId: string) => void) | undefined;
    client.start.mockReturnValue(
      new Promise<string>((resolve) => {
        resolveStart = resolve;
      }),
    );
    render(<AssistantPanel />);
    const composer = await screen.findByRole("textbox", {
      name: "Ask for assistance",
    });
    fireEvent.change(composer, { target: { value: "Draft a response" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(client.start).toHaveBeenCalledTimes(1);
    });
    await act(async () => {
      receiveEvent?.({ type: "started", requestId: "request-1" });
      receiveEvent?.({
        type: "textDelta",
        requestId: "stale-request",
        delta: "Stale text",
      });
      receiveEvent?.({
        type: "textDelta",
        requestId: "request-1",
        delta: "A useful answer",
      });
      resolveStart?.("request-1");
      await Promise.resolve();
    });

    expect(screen.getByText("A useful answer")).toBeInTheDocument();
    expect(screen.queryByText("Stale text")).not.toBeInTheDocument();

    act(() => {
      receiveEvent?.({
        type: "completed",
        requestId: "request-1",
        provider: "open_router",
        model: "openrouter/free",
        usage: null,
      });
    });

    await waitFor(() => expect(composer).toHaveFocus());
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("keeps both sides of each exchange visible without sending prior turns again", async () => {
    client.start.mockResolvedValueOnce("request-5").mockResolvedValueOnce("request-6");
    render(<AssistantPanel />);
    const composer = await screen.findByRole("textbox", {
      name: "Ask for assistance",
    });

    fireEvent.change(composer, { target: { value: "First question" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(client.start).toHaveBeenNthCalledWith(1, "First question");
    });
    expect(composer).toHaveValue("");
    await screen.findByRole("button", { name: "Cancel" });
    await act(async () => {
      receiveEvent?.({ type: "started", requestId: "request-5" });
      receiveEvent?.({
        type: "textDelta",
        requestId: "request-5",
        delta: "First answer",
      });
      receiveEvent?.({
        type: "completed",
        requestId: "request-5",
        provider: "open_router",
        model: "openrouter/free",
        usage: null,
      });
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    });

    fireEvent.change(composer, { target: { value: "Follow-up question" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(client.start).toHaveBeenNthCalledWith(2, "Follow-up question");
    });
    await screen.findByRole("button", { name: "Cancel" });
    await act(async () => {
      receiveEvent?.({
        type: "textDelta",
        requestId: "request-6",
        delta: "Follow-up answer",
      });
      await Promise.resolve();
    });

    expect(screen.getByText("First question")).toBeInTheDocument();
    expect(screen.getByText("First answer")).toBeInTheDocument();
    expect(screen.getByText("Follow-up question")).toBeInTheDocument();
    expect(screen.getByText("Follow-up answer")).toBeInTheDocument();
  });

  it("renders fenced code in assistant responses as formatted code blocks", async () => {
    client.start.mockResolvedValue("request-7");
    const { container } = render(<AssistantPanel />);
    const composer = await screen.findByRole("textbox", {
      name: "Ask for assistance",
    });
    fireEvent.change(composer, { target: { value: "Write a Python calculator" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(client.start).toHaveBeenCalledTimes(1);
    });
    await screen.findByRole("button", { name: "Cancel" });

    await act(async () => {
      receiveEvent?.({
        type: "textDelta",
        requestId: "request-7",
        delta:
          "Here is the calculation:\n\n```python\nprint(1 + 1)\n```\n\n**It prints two.**",
      });
      await Promise.resolve();
    });

    expect(container.querySelector("pre code.language-python")).toHaveTextContent(
      "print(1 + 1)",
    );
    expect(screen.getByText("It prints two.").tagName).toBe("STRONG");
  });

  it("submits with Enter, preserves Shift+Enter, and prevents duplicate requests", async () => {
    let resolveStart: ((requestId: string) => void) | undefined;
    client.start.mockReturnValue(
      new Promise<string>((resolve) => {
        resolveStart = resolve;
      }),
    );
    render(<AssistantPanel />);
    const composer = await screen.findByRole("textbox", {
      name: "Ask for assistance",
    });
    fireEvent.change(composer, { target: { value: "Two lines" } });

    fireEvent.keyDown(composer, { key: "Enter", shiftKey: true });
    expect(client.start).not.toHaveBeenCalled();
    fireEvent.keyDown(composer, { key: "Enter" });
    fireEvent.keyDown(composer, { key: "Enter" });

    expect(client.start).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Starting" })).toBeDisabled();
    act(() => {
      resolveStart?.("request-2");
    });
  });

  it("cancels the current request and recovers after a safe failure", async () => {
    const currentRequestId = "request-3";
    const nextRequestId = "request-4";
    client.start
      .mockResolvedValueOnce(currentRequestId)
      .mockResolvedValueOnce(nextRequestId);
    render(<AssistantPanel />);
    const composer = await screen.findByRole("textbox", {
      name: "Ask for assistance",
    });
    fireEvent.change(composer, { target: { value: "Help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(client.start).toHaveBeenCalled();
    });
    act(() => {
      receiveEvent?.({ type: "started", requestId: currentRequestId });
      receiveEvent?.({
        type: "textDelta",
        requestId: currentRequestId,
        delta: "Partial response",
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(client.cancel).toHaveBeenCalledWith(currentRequestId);
    act(() => {
      receiveEvent?.({ type: "cancelled", requestId: currentRequestId });
    });
    expect(screen.getByText("Request cancelled")).toBeInTheDocument();
    expect(screen.getByText("Partial response")).toBeInTheDocument();

    fireEvent.change(composer, { target: { value: "Try again" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(client.start).toHaveBeenCalledTimes(2);
    });
    act(() => {
      receiveEvent?.({ type: "started", requestId: nextRequestId });
      receiveEvent?.({
        type: "textDelta",
        requestId: nextRequestId,
        delta: "Partial response",
      });
      receiveEvent?.({
        type: "failed",
        requestId: nextRequestId,
        code: "rateLimit",
        message: "OpenRouter rate limit reached; try again later",
      });
    });

    expect(
      screen.getByText("OpenRouter rate limit reached; try again later"),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Partial response")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("shows honest configuration guidance without exposing a composer", async () => {
    client.getReadiness.mockResolvedValue({
      status: "unconfigured",
      message: "Required setting SINGULARITY_LIVE_MODEL is not configured",
    });

    render(<AssistantPanel />);

    expect(
      await screen.findByText(
        "Required setting SINGULARITY_LIVE_MODEL is not configured",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("shows an unavailable state when the event channel cannot be opened", async () => {
    client.subscribe.mockRejectedValue(new Error("Outside the desktop shell"));

    render(<AssistantPanel />);

    expect(
      await screen.findByText("Provider status is unavailable"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
});
