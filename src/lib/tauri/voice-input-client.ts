import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AudioInputSource = "microphone" | "system_audio";

export interface AudioInputDevice {
  readonly id: string;
  readonly label: string;
  readonly isDefault: boolean;
}

export interface VoiceInputSettings {
  readonly source: AudioInputSource;
  readonly microphoneDeviceId: string | null;
}

export type VoiceInputEvent =
  | { readonly type: "started"; readonly source: AudioInputSource }
  | { readonly type: "sourceChanged"; readonly source: AudioInputSource }
  | { readonly type: "sourceChangeFailed"; readonly message: string }
  | { readonly type: "transcript"; readonly text: string }
  | { readonly type: "stopped"; readonly transcript: string }
  | { readonly type: "failed"; readonly message: string };

export async function getAudioInputDevices(): Promise<readonly AudioInputDevice[]> {
  const response: unknown = await invoke("list_audio_input_devices");
  if (!Array.isArray(response)) {
    throw new Error("Invalid audio input device response");
  }
  const devices = response.flatMap((value): AudioInputDevice[] => {
    if (
      isRecord(value) &&
      Object.keys(value).length === 3 &&
      Object.hasOwn(value, "id") &&
      Object.hasOwn(value, "label") &&
      Object.hasOwn(value, "isDefault") &&
      typeof value.id === "string" &&
      value.id.length > 0 &&
      typeof value.label === "string" &&
      value.label.length > 0 &&
      typeof value.isDefault === "boolean"
    ) {
      return [{ id: value.id, label: value.label, isDefault: value.isDefault }];
    }
    return [];
  });
  if (devices.length !== response.length) {
    throw new Error("Invalid audio input device response");
  }
  return devices;
}

export async function getVoiceInputSettings(): Promise<VoiceInputSettings> {
  const response: unknown = await invoke("get_voice_input_settings");
  if (
    !isRecord(response) ||
    Object.keys(response).length !== 2 ||
    !Object.hasOwn(response, "source") ||
    !Object.hasOwn(response, "microphoneDeviceId") ||
    (response.source !== "microphone" && response.source !== "system_audio") ||
    (response.microphoneDeviceId !== null &&
      typeof response.microphoneDeviceId !== "string")
  ) {
    throw new Error("Invalid voice input settings response");
  }
  return {
    source: response.source,
    microphoneDeviceId: response.microphoneDeviceId,
  };
}

export async function setVoiceInputSource(
  source: AudioInputSource,
  microphoneDeviceId: string | null,
): Promise<void> {
  await invoke("set_voice_input_source", {
    request: { source, microphoneDeviceId },
  });
}

export async function startVoiceInput(
  source: AudioInputSource,
  microphoneDeviceId: string | null,
): Promise<void> {
  await invoke("start_voice_input", {
    request: { source, microphoneDeviceId },
  });
}

export async function stopVoiceInput(): Promise<void> {
  await invoke("stop_voice_input");
}

export async function subscribeVoiceInputEvents(
  onEvent: (event: VoiceInputEvent) => void,
): Promise<() => void> {
  return listen<unknown>("singularity:voice-input", ({ payload }) => {
    const event = parseVoiceInputEvent(payload);
    if (event !== null) {
      onEvent(event);
    }
  });
}

function parseVoiceInputEvent(value: unknown): VoiceInputEvent | null {
  if (!isRecord(value) || typeof value.type !== "string") {
    return null;
  }
  if (
    value.type === "started" &&
    (value.source === "microphone" || value.source === "system_audio")
  ) {
    return { type: "started", source: value.source };
  }
  if (
    value.type === "sourceChanged" &&
    (value.source === "microphone" || value.source === "system_audio")
  ) {
    return { type: "sourceChanged", source: value.source };
  }
  if (value.type === "sourceChangeFailed" && typeof value.message === "string") {
    return { type: "sourceChangeFailed", message: value.message };
  }
  if (value.type === "transcript" && typeof value.text === "string") {
    return { type: "transcript", text: value.text };
  }
  if (value.type === "stopped" && typeof value.transcript === "string") {
    return { type: "stopped", transcript: value.transcript };
  }
  if (value.type === "failed" && typeof value.message === "string") {
    return { type: "failed", message: value.message };
  }
  return null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
