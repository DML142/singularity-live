import { spawn as spawnProcess } from "node:child_process";
import { createInterface } from "node:readline/promises";
import { pathToFileURL } from "node:url";

const DEFAULT_MODEL = "openrouter/free";
const DEFAULT_TIMEOUT_SECONDS = "60";
const TASKS = [
  { value: "text", label: "Manual text assistance" },
  { value: "screenshot", label: "Screenshot assistance" },
];

export async function launchDevAssist({
  argv,
  env,
  promptChoice,
  promptText,
  promptSecret,
  spawn,
  writeError,
}) {
  try {
    const values =
      argv.length === 0
        ? await collectInteractiveValues(env, {
            promptChoice,
            promptText,
            promptSecret,
          })
        : parseArgumentValues(argv, env);
    validateValues(values);
    const childEnvironment = {
      ...env,
      OPENROUTER_API_KEY: values.apiKey,
      SINGULARITY_LIVE_PROVIDER: "openrouter",
      SINGULARITY_LIVE_MODEL: values.model,
      SINGULARITY_LIVE_CONTEXT_PACK: values.contextPack,
      SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS: values.timeoutSeconds,
      SINGULARITY_LIVE_DEV_ASSIST_TASK: values.task,
      VITE_SINGULARITY_LIVE_DEV_ASSIST_TASK: values.task,
    };
    return await spawn("pnpm", ["tauri", "dev"], {
      env: childEnvironment,
      stdio: "inherit",
    });
  } catch (error) {
    writeError(error instanceof Error ? error.message : "Development launcher failed");
    return 1;
  }
}

export async function readHiddenInput(input, output, prompt) {
  if (!input.isTTY || typeof input.setRawMode !== "function") {
    throw new Error("A terminal is required to enter a missing API key safely");
  }
  output.write(prompt);
  input.setEncoding("utf8");
  input.setRawMode(true);
  input.resume();
  return new Promise((resolve, reject) => {
    let value = "";
    const finish = (result, error) => {
      input.off("data", onData);
      input.off("error", onError);
      input.setRawMode(false);
      input.pause();
      output.write("\n");
      if (error) {
        reject(error);
      } else {
        resolve(result);
      }
    };
    const onError = () => finish(undefined, new Error("Secret input failed"));
    const onData = (chunk) => {
      for (const character of String(chunk)) {
        if (character === "\r" || character === "\n") {
          finish(value);
          return;
        }
        if (character === "\u0003" || character === "\u0004") {
          finish(undefined, new Error("Secret input cancelled"));
          return;
        }
        if (character === "\u007f" || character === "\b") {
          value = value.slice(0, -1);
        } else {
          value += character;
        }
      }
    };
    input.on("data", onData);
    input.on("error", onError);
  });
}

async function collectInteractiveValues(env, prompts) {
  const task = await prompts.promptChoice("Choose an assistance workflow:", TASKS);
  const model =
    env.SINGULARITY_LIVE_MODEL ||
    (await prompts.promptText("OpenRouter model slug", DEFAULT_MODEL));
  const contextPack =
    env.SINGULARITY_LIVE_CONTEXT_PACK ||
    (await prompts.promptText("Installed context pack ID", ""));
  const apiKey =
    env.OPENROUTER_API_KEY ||
    (await prompts.promptSecret("OpenRouter API key (input hidden): "));
  return {
    task,
    model,
    contextPack,
    apiKey,
    timeoutSeconds:
      env.SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS || DEFAULT_TIMEOUT_SECONDS,
  };
}

function parseArgumentValues(argv, env) {
  const options = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    if (option === "--help") {
      throw new Error(
        "Usage: pnpm assist --task <text|screenshot> [--model <slug>] [--context-pack <id>]",
      );
    }
    if (
      !["--task", "--model", "--context-pack", "--timeout-seconds"].includes(option)
    ) {
      throw new Error(
        "Unknown or unsupported option; API keys must come from the environment",
      );
    }
    const value = argv[index + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new Error("A launcher option is missing its value");
    }
    if (options.has(option)) {
      throw new Error("Launcher options may be supplied only once");
    }
    if (looksLikeCredential(value)) {
      throw new Error("A launcher option value is invalid");
    }
    options.set(option, value);
    index += 1;
  }
  const task = options.get("--task") || env.SINGULARITY_LIVE_DEV_ASSIST_TASK;
  if (!task) {
    throw new Error("Argument mode requires --task text or --task screenshot");
  }
  const apiKey = env.OPENROUTER_API_KEY;
  if (!apiKey || apiKey.trim().length === 0) {
    throw new Error(
      "Set OPENROUTER_API_KEY in the environment before using argument mode",
    );
  }
  return {
    task,
    model: options.get("--model") || env.SINGULARITY_LIVE_MODEL || DEFAULT_MODEL,
    contextPack:
      options.get("--context-pack") || env.SINGULARITY_LIVE_CONTEXT_PACK || "",
    apiKey,
    timeoutSeconds:
      options.get("--timeout-seconds") ||
      env.SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS ||
      DEFAULT_TIMEOUT_SECONDS,
  };
}

function validateValues(values) {
  if (!TASKS.some((task) => task.value === values.task)) {
    throw new Error("Choose a supported assistance workflow");
  }
  if (
    !/^[A-Za-z0-9][A-Za-z0-9._:/-]{0,159}$/.test(values.model) ||
    looksLikeCredential(values.model)
  ) {
    throw new Error("The OpenRouter model slug is invalid");
  }
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(values.contextPack)) {
    throw new Error("Set the ID of an installed context pack");
  }
  if (!/^(?:[5-9]|[1-9][0-9]|[12][0-9]{2}|300)$/.test(values.timeoutSeconds)) {
    throw new Error("Request timeout must be an integer from 5 to 300 seconds");
  }
  if (typeof values.apiKey !== "string" || values.apiKey.trim().length === 0) {
    throw new Error("Set OPENROUTER_API_KEY before launching the app");
  }
}

function looksLikeCredential(value) {
  return (
    /^(?:sk-|or-)[A-Za-z0-9_-]{12,}$/i.test(value) || /^[A-Za-z0-9_-]{48,}$/.test(value)
  );
}

function promptChoice(prompt, choices) {
  return withReadline(async (readline) => {
    while (true) {
      process.stdout.write(`${prompt}\n`);
      for (const [index, choice] of choices.entries()) {
        process.stdout.write(`  ${index + 1}) ${choice.label}\n`);
      }
      const answer = (await readline.question("Select a number: ")).trim();
      const choice = choices[Number(answer) - 1];
      if (choice) {
        return choice.value;
      }
      process.stdout.write("Choose one of the listed numbers.\n");
    }
  });
}

function promptText(prompt, defaultValue) {
  return withReadline(async (readline) => {
    const suffix = defaultValue ? ` [${defaultValue}]` : "";
    const answer = (await readline.question(`${prompt}${suffix}: `)).trim();
    return answer || defaultValue;
  });
}

function promptSecret(prompt) {
  return readHiddenInput(process.stdin, process.stdout, prompt);
}

function withReadline(callback) {
  const readline = createInterface({ input: process.stdin, output: process.stdout });
  return callback(readline).finally(() => readline.close());
}

function spawnDevProcess(command, args, options) {
  return new Promise((resolve, reject) => {
    const child = spawnProcess(command, args, options);
    child.once("error", () =>
      reject(new Error("The Tauri development process could not start")),
    );
    child.once("exit", (code) => resolve(code ?? 1));
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const exitCode = await launchDevAssist({
    argv: process.argv.slice(2),
    env: process.env,
    promptChoice,
    promptText,
    promptSecret,
    spawn: spawnDevProcess,
    writeError: (message) => process.stderr.write(`${message}\n`),
  });
  process.exitCode = exitCode;
}
