import { invoke } from "@tauri-apps/api/core";

export interface UserContextFileInfo {
  readonly id: string;
  readonly name: string;
}

export async function getUserContextFiles(): Promise<readonly UserContextFileInfo[]> {
  return parseUserContextFiles(await invoke<unknown>("get_user_context_files"));
}

export async function addUserContextFiles(): Promise<readonly UserContextFileInfo[]> {
  return parseUserContextFiles(await invoke<unknown>("add_user_context_files"));
}

export async function removeUserContextFile(
  id: string,
): Promise<readonly UserContextFileInfo[]> {
  return parseUserContextFiles(
    await invoke<unknown>("remove_user_context_file", { request: { id } }),
  );
}

function parseUserContextFiles(value: unknown): readonly UserContextFileInfo[] {
  if (!Array.isArray(value)) {
    throw new Error("Invalid context file list response");
  }
  return value.map((item) => {
    if (
      !isRecord(item) ||
      typeof item.id !== "string" ||
      item.id.trim().length === 0 ||
      typeof item.name !== "string" ||
      item.name.trim().length === 0
    ) {
      throw new Error("Invalid context file list response");
    }
    return { id: item.id, name: item.name };
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
