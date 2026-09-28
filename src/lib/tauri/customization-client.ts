import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_WINDOW_OPACITY = 100;
export const MIN_WINDOW_OPACITY = 40;

export async function getWindowOpacity(): Promise<number> {
  return parseWindowOpacity(await invoke<unknown>("get_window_opacity"));
}

export async function setWindowOpacity(percentage: number): Promise<number> {
  if (!isSupportedOpacity(percentage)) {
    throw new Error("Invalid window opacity");
  }
  return parseWindowOpacity(
    await invoke<unknown>("set_window_opacity", { request: { percentage } }),
  );
}

export function isSupportedOpacity(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= MIN_WINDOW_OPACITY &&
    value <= 100 &&
    value % 5 === 0
  );
}

function parseWindowOpacity(value: unknown): number {
  if (!isSupportedOpacity(value)) {
    throw new Error("Invalid window opacity response");
  }
  return value;
}
