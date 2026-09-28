import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  addUserContextFiles,
  getUserContextFiles,
  removeUserContextFile,
} from "./user-context-client";

describe("user context file IPC client", () => {
  beforeEach(() => invokeMock.mockReset());

  it("lists, imports, and removes context files through typed commands", async () => {
    const files = [{ id: "file-id", name: "notes.md" }];
    invokeMock.mockResolvedValue(files);

    await expect(getUserContextFiles()).resolves.toEqual(files);
    await expect(addUserContextFiles()).resolves.toEqual(files);
    await expect(removeUserContextFile("file-id")).resolves.toEqual(files);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_user_context_files");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "add_user_context_files");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "remove_user_context_file", {
      request: { id: "file-id" },
    });
  });

  it("rejects malformed file metadata from Rust", async () => {
    invokeMock.mockResolvedValue([{ id: "file-id", name: " " }]);

    await expect(getUserContextFiles()).rejects.toThrow(
      "Invalid context file list response",
    );
  });
});
