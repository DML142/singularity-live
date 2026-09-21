import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("application shell", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      applicationName: "Singularity Live",
      version: "0.1.0",
      backendState: "ready",
    });
  });

  it("shows the real backend status and honest empty workspace states", async () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { level: 1, name: "Singularity Live" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Backend ready")).toBeInTheDocument();
    expect(screen.getByText("No active session")).toBeInTheDocument();
    expect(
      screen.getByText("Suggestions will appear when a session is active."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toBeDisabled();
  });

  it("reports an unavailable backend when IPC returns a malformed payload", async () => {
    invokeMock.mockResolvedValue({});

    render(<App />);

    expect(await screen.findByText("Backend unavailable")).toBeInTheDocument();
  });

  it("does not report readiness when the backend state is not ready", async () => {
    invokeMock.mockResolvedValue({
      applicationName: "Singularity Live",
      version: "0.1.0",
      backendState: "starting",
    });

    render(<App />);

    expect(await screen.findByText("Backend unavailable")).toBeInTheDocument();
  });
});
