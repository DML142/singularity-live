import { EventEmitter } from "node:events";

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  launchDevAssist,
  readHiddenInput,
  type DevAssistOptions,
} from "./dev-assist.mjs";

describe("development assist launcher", () => {
  const promptChoice = vi.fn<DevAssistOptions["promptChoice"]>();
  const promptMenu = vi.fn<NonNullable<DevAssistOptions["promptMenu"]>>();
  const promptText = vi.fn<DevAssistOptions["promptText"]>();
  const promptSecret = vi.fn<DevAssistOptions["promptSecret"]>();
  const spawn = vi.fn<DevAssistOptions["spawn"]>();
  const writeError = vi.fn<DevAssistOptions["writeError"]>();

  beforeEach(() => {
    promptChoice.mockReset().mockResolvedValue("screenshot");
    promptMenu.mockReset().mockResolvedValue("gpt-6-luna");
    promptText.mockReset().mockResolvedValue("fictional-developer");
    promptSecret.mockReset().mockResolvedValue("test-openrouter-secret");
    spawn.mockReset().mockResolvedValue(0);
    writeError.mockReset();
  });

  it("offers a task and passes a hidden key only to the child environment", async () => {
    const result = await launchDevAssist({
      argv: [],
      env: {},
      platform: "linux",
      promptChoice,
      promptText,
      promptSecret,
      spawn,
      writeError,
    });

    expect(result).toBe(0);
    expect(promptChoice).toHaveBeenCalledOnce();
    expect(promptSecret).toHaveBeenCalledTimes(2);
    const spawnedCall = spawn.mock.calls.at(0);
    expect(spawnedCall).toBeDefined();
    if (spawnedCall === undefined) {
      throw new Error("The dev process was not started");
    }
    const [command, args, options] = spawnedCall;
    expect(command).toBe("pnpm");
    expect(args).toEqual(["tauri", "dev"]);
    expect(options.env.OPENROUTER_API_KEY).toBe("test-openrouter-secret");
    expect(options.env.SONIOX_API_KEY).toBe("test-openrouter-secret");
    expect(options.env.SINGULARITY_LIVE_DEV_ASSIST_TASK).toBe("screenshot");
    expect(options.env.VITE_SINGULARITY_LIVE_DEV_ASSIST_TASK).toBe("screenshot");
    expect(writeError).not.toHaveBeenCalledWith(
      expect.stringContaining("test-openrouter-secret"),
    );
  });

  it("offers GPT-6 Luna and passes both provider keys only to the child process", async () => {
    promptSecret
      .mockReset()
      .mockResolvedValueOnce("test-openai-secret")
      .mockResolvedValueOnce("test-soniox-secret");

    const result = await launchDevAssist({
      argv: [],
      env: { SINGULARITY_LIVE_CONTEXT_PACK: "fictional-developer" },
      platform: "linux",
      promptChoice,
      promptMenu,
      promptText,
      promptSecret,
      spawn,
      writeError,
    });

    expect(result).toBe(0);
    expect(promptMenu).toHaveBeenCalledOnce();
    const spawnedCall = spawn.mock.calls.at(0);
    expect(spawnedCall).toBeDefined();
    if (spawnedCall === undefined) {
      throw new Error("The dev process was not started");
    }
    const [, , options] = spawnedCall;
    expect(options.env.OPENAI_API_KEY).toBe("test-openai-secret");
    expect(options.env.SONIOX_API_KEY).toBe("test-soniox-secret");
    expect(options.env.SINGULARITY_LIVE_PROVIDER).toBe("openai");
    expect(options.env.SINGULARITY_LIVE_MODEL).toBe("gpt-6-luna");
    expect(writeError).not.toHaveBeenCalled();
  });

  it("starts the Tauri development command through the Windows command processor", async () => {
    const result = await launchDevAssist({
      argv: [],
      env: { ComSpec: "C:\\Windows\\System32\\cmd.exe" },
      platform: "win32",
      promptChoice,
      promptText,
      promptSecret,
      spawn,
      writeError,
    });

    expect(result).toBe(0);
    const spawnedCall = spawn.mock.calls.at(0);
    expect(spawnedCall).toBeDefined();
    if (spawnedCall === undefined) {
      throw new Error("The dev process was not started");
    }
    const [command, args] = spawnedCall;
    expect(command).toBe("C:\\Windows\\System32\\cmd.exe");
    expect(args).toEqual(["/d", "/s", "/c", "pnpm tauri dev"]);
  });

  it("skips prompts in argument mode and refuses a missing environment key", async () => {
    const result = await launchDevAssist({
      argv: ["--task", "text", "--context-pack", "fictional-developer"],
      env: {},
      promptChoice,
      promptText,
      promptSecret,
      spawn,
      writeError,
    });

    expect(result).toBe(1);
    expect(promptChoice).not.toHaveBeenCalled();
    expect(promptText).not.toHaveBeenCalled();
    expect(promptSecret).not.toHaveBeenCalled();
    expect(spawn).not.toHaveBeenCalled();
    expect(writeError).toHaveBeenCalledOnce();
  });

  it("launches explicit arguments from env config without opening any prompt", async () => {
    const result = await launchDevAssist({
      argv: ["--task", "text", "--context-pack", "fictional-developer"],
      env: { OPENROUTER_API_KEY: "configured-secret" },
      promptChoice,
      promptText,
      promptSecret,
      spawn,
      writeError,
    });

    expect(result).toBe(0);
    expect(promptChoice).not.toHaveBeenCalled();
    expect(promptText).not.toHaveBeenCalled();
    expect(promptSecret).not.toHaveBeenCalled();
    expect(spawn).toHaveBeenCalledOnce();
  });

  it("never accepts key options or key-looking positional values", async () => {
    for (const argv of [
      [
        "--task",
        "text",
        "--context-pack",
        "fictional-developer",
        "--api-key",
        "secret",
      ],
      ["sk-or-v1-sensitive-looking-value"],
    ]) {
      const result = await launchDevAssist({
        argv,
        env: { OPENROUTER_API_KEY: "configured-secret" },
        promptChoice,
        promptText,
        promptSecret,
        spawn,
        writeError,
      });
      expect(result).toBe(1);
    }
    expect(spawn).not.toHaveBeenCalled();
    expect(writeError.mock.calls.flat().join(" ")).not.toContain(
      "sensitive-looking-value",
    );
  });

  it("reads secret input without echo and always restores terminal mode", async () => {
    class FakeInput extends EventEmitter {
      isTTY = true;
      rawModes: boolean[] = [];
      setRawMode(enabled: boolean) {
        this.rawModes.push(enabled);
        return this;
      }
      setEncoding() {
        return this;
      }
      resume() {
        return this;
      }
      pause() {
        return this;
      }
    }
    const input = new FakeInput();
    const output = {
      text: "",
      write(chunk: string) {
        this.text += chunk;
      },
    };
    const reading = readHiddenInput(input, output, "OpenRouter key: ");
    input.emit("data", "test-openrouter-secret");
    input.emit("data", "\r");

    await expect(reading).resolves.toBe("test-openrouter-secret");
    expect(output.text).not.toContain("test-openrouter-secret");
    expect(input.rawModes).toEqual([true, false]);
  });
});
