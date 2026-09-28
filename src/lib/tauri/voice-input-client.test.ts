import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import {
  getAudioInputDevices,
  getVoiceInputSettings,
  setVoiceInputSource,
  startVoiceInput,
} from "./voice-input-client";

describe("voice input IPC client", () => {
  beforeEach(() => invokeMock.mockReset());

  it("strictly validates and lists microphone endpoints", async () => {
    const devices = [{ id: "endpoint-1", label: "USB microphone", isDefault: true }];
    invokeMock.mockResolvedValue(devices);

    await expect(getAudioInputDevices()).resolves.toEqual(devices);
    expect(invokeMock).toHaveBeenCalledWith("list_audio_input_devices");

    invokeMock.mockResolvedValue([{ ...devices[0], apiKey: "must-not-cross-ipc" }]);
    await expect(getAudioInputDevices()).rejects.toThrow(
      "Invalid audio input device response",
    );
  });

  it("loads the persisted audio source and selected microphone", async () => {
    invokeMock.mockResolvedValue({
      source: "microphone",
      microphoneDeviceId: "endpoint-1",
    });

    await expect(getVoiceInputSettings()).resolves.toEqual({
      source: "microphone",
      microphoneDeviceId: "endpoint-1",
    });
    expect(invokeMock).toHaveBeenCalledWith("get_voice_input_settings");
  });

  it("passes the selected microphone endpoint to Rust for source selection and capture", async () => {
    invokeMock.mockResolvedValue(undefined);

    await setVoiceInputSource("microphone", "endpoint-1");
    await startVoiceInput("microphone", "endpoint-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "set_voice_input_source", {
      request: { source: "microphone", microphoneDeviceId: "endpoint-1" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "start_voice_input", {
      request: { source: "microphone", microphoneDeviceId: "endpoint-1" },
    });
  });
});
