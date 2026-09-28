import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { getAppScale, setAppScale } from "./customization-client";

describe("application scale IPC client", () => {
  beforeEach(() => invokeMock.mockReset());

  it("loads and saves supported browser-style zoom values", async () => {
    invokeMock.mockResolvedValueOnce(100).mockResolvedValueOnce(110);

    await expect(getAppScale()).resolves.toBe(100);
    await expect(setAppScale(110)).resolves.toBe(110);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_app_scale");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "set_app_scale", {
      request: { percentage: 110 },
    });
  });

  it("rejects unsupported scale values before invoking Rust", async () => {
    await expect(setAppScale(95)).rejects.toThrow("Invalid application scale");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
