import { beforeEach, describe, expect, it } from "vitest";

import { useVoiceInputStore } from "./voice-input-store";

describe("voice input store source shortcuts", () => {
  beforeEach(() => {
    useVoiceInputStore.setState({
      phase: "recording",
      source: "microphone",
      microphoneDeviceId: "endpoint-1",
      error: null,
    });
  });

  it("updates the selected source after a global source-switch bind", () => {
    useVoiceInputStore
      .getState()
      .handleEvent({ type: "sourceChanged", source: "system_audio" });

    expect(useVoiceInputStore.getState().source).toBe("system_audio");
  });

  it("reports a rejected source switch without ending an active recording", () => {
    useVoiceInputStore.getState().handleEvent({
      type: "sourceChangeFailed",
      message: "Voice input is already active",
    });

    expect(useVoiceInputStore.getState().phase).toBe("recording");
    expect(useVoiceInputStore.getState().error).toBe("Voice input is already active");
  });
});
