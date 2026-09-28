import { invoke } from "@tauri-apps/api/core";

export type ProviderId = "open_router" | "open_ai" | "gemini";

export interface ProviderKeyStatus {
  readonly openai: "configured" | "missing";
  readonly openrouter: "configured" | "missing";
  readonly gemini: "configured" | "missing";
  readonly soniox: "configured" | "missing";
}

export interface ProviderSettings {
  readonly provider: ProviderId;
  readonly model: string;
  readonly contextPack: string;
  readonly keys: ProviderKeyStatus;
  readonly filePath: string;
}

export async function getProviderSettings(): Promise<ProviderSettings> {
  return parseSettings(await invoke<unknown>("get_provider_settings"));
}

export async function saveProviderProfile(
  provider: ProviderId,
  model: string,
): Promise<ProviderSettings> {
  return parseSettings(
    await invoke<unknown>("save_provider_profile", {
      request: { provider, model },
    }),
  );
}

export async function openProviderSettingsFile(): Promise<void> {
  await invoke("open_provider_settings_file");
}

function parseSettings(value: unknown): ProviderSettings {
  if (
    !isRecord(value) ||
    !isProviderId(value.provider) ||
    typeof value.model !== "string" ||
    typeof value.contextPack !== "string" ||
    typeof value.filePath !== "string" ||
    !isRecord(value.keys) ||
    !isKeyPresence(value.keys.openai) ||
    !isKeyPresence(value.keys.openrouter) ||
    !isKeyPresence(value.keys.gemini) ||
    !isKeyPresence(value.keys.soniox)
  ) {
    throw new Error("Invalid provider settings response");
  }
  return {
    provider: value.provider,
    model: value.model,
    contextPack: value.contextPack,
    filePath: value.filePath,
    keys: {
      openai: value.keys.openai,
      openrouter: value.keys.openrouter,
      gemini: value.keys.gemini,
      soniox: value.keys.soniox,
    },
  };
}

function isProviderId(value: unknown): value is ProviderId {
  return value === "open_router" || value === "open_ai" || value === "gemini";
}

function isKeyPresence(value: unknown): value is "configured" | "missing" {
  return value === "configured" || value === "missing";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
