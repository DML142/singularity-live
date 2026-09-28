import { invoke } from "@tauri-apps/api/core";

import type { CaptureTargetKind } from "./screen-assistance-client";

export const DEFAULT_WINDOW_OPACITY = 100;
export const MIN_WINDOW_OPACITY = 40;

export interface ScreenshotPreferences {
  readonly closeWindowOnScreenshot: boolean;
  readonly targetKind: CaptureTargetKind;
}

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

export async function getScreenshotPreferences(): Promise<ScreenshotPreferences> {
  return parseScreenshotPreferences(
    await invoke<unknown>("get_screenshot_preferences"),
  );
}

export async function setScreenshotPreferences(
  preferences: ScreenshotPreferences,
): Promise<ScreenshotPreferences> {
  return parseScreenshotPreferences(
    await invoke<unknown>("set_screenshot_preferences", { request: preferences }),
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

function parseScreenshotPreferences(value: unknown): ScreenshotPreferences {
  if (
    !isRecord(value) ||
    !Object.hasOwn(value, "closeWindowOnScreenshot") ||
    !Object.hasOwn(value, "targetKind")
  ) {
    throw new Error("Invalid screenshot preferences response");
  }
  if (
    typeof value.closeWindowOnScreenshot !== "boolean" ||
    (value.targetKind !== "monitor" && value.targetKind !== "window")
  ) {
    throw new Error("Invalid screenshot preferences response");
  }
  return {
    closeWindowOnScreenshot: value.closeWindowOnScreenshot,
    targetKind: value.targetKind,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
