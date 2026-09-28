import { spawn as spawnProcess } from "node:child_process";
import { readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";
import { pathToFileURL } from "node:url";

const DEFAULT_MODEL = "google/gemma-4-31b-it:free";
const DEFAULT_GEMINI_MODEL = "gemini-3.8-flash";
const DEFAULT_OPENAI_MODEL = "gpt-6-luna";
const DEFAULT_TIMEOUT_SECONDS = "60";
const MODEL_PROFILES = [
  {
    value: "gpt-6-luna",
    label: "OpenAI — GPT-6 Luna (text and screenshots)",
    provider: "openai",
    model: DEFAULT_OPENAI_MODEL,
  },
  {
    value: "gemini-3.8-flash",
    label: "Google AI Studio — Gemini 3.8 Flash (text and screenshots)",
    provider: "gemini",
    model: DEFAULT_GEMINI_MODEL,
  },
  {
    value: "openrouter/free",
    label: "OpenRouter — free model router",
    provider: "openrouter",
    model: "openrouter/free",
  },
];
const TASKS = [
  { value: "text", label: "Manual text assistance" },
  { value: "screenshot", label: "Screenshot assistance" },
];

export async function launchDevAssist({
  argv,
  env,
  promptChoice,
  promptMenu,
  promptText,
  promptSecret,
  spawn,
  writeError,
  platform = process.platform,
}) {
  try {
    const values =
      argv.length === 0
        ? await collectInteractiveValues(env, platform, {
            promptChoice,
            promptMenu,
            promptText,
            promptSecret,
          })
        : parseArgumentValues(argv, env);
    validateValues(values);
    const childEnvironment = {
      ...env,
      [apiKeyName(values.provider)]: values.apiKey,
      ...(values.sonioxApiKey ? { SONIOX_API_KEY: values.sonioxApiKey } : {}),
      SINGULARITY_LIVE_PROVIDER: values.provider,
      SINGULARITY_LIVE_MODEL: values.model,
      SINGULARITY_LIVE_CONTEXT_PACK: values.contextPack,
      SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS: values.timeoutSeconds,
      SINGULARITY_LIVE_DEV_ASSIST_TASK: values.task,
      VITE_SINGULARITY_LIVE_DEV_ASSIST_TASK: values.task,
    };
    const invocation =
      platform === "win32"
        ? {
            command: env.ComSpec || env.COMSPEC || "cmd.exe",
            args: ["/d", "/s", "/c", "pnpm tauri dev"],
          }
        : { command: "pnpm", args: ["tauri", "dev"] };
    return await spawn(invocation.command, invocation.args, {
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

async function collectInteractiveValues(env, platform, prompts) {
  const task = await prompts.promptChoice("Choose an assistance workflow:", TASKS);
  const configuredProvider = env.SINGULARITY_LIVE_PROVIDER;
  let provider = configuredProvider;
  let model = env.SINGULARITY_LIVE_MODEL;
  if (!model && prompts.promptMenu) {
    const profiles = MODEL_PROFILES.filter(
      (profile) => !configuredProvider || profile.provider === configuredProvider,
    );
    if (profiles.length === 0) {
      throw new Error("No numbered model choices are available for this provider");
    }
    const selectedProfile = await prompts.promptMenu(
      "Choose a provider and model:",
      profiles.map(({ value, label }) => ({ value, label })),
    );
    const profile = profiles.find((candidate) => candidate.value === selectedProfile);
    if (!profile) {
      throw new Error("Choose one of the listed provider and model options");
    }
    provider = profile.provider;
    model = profile.model;
  } else if (!model) {
    model = await prompts.promptText(
      "Model ID (provider model ID)",
      configuredProvider === "gemini"
        ? DEFAULT_GEMINI_MODEL
        : configuredProvider === "openai"
          ? DEFAULT_OPENAI_MODEL
          : DEFAULT_MODEL,
    );
  }
  provider ||= inferProvider(model);

  let contextPack = env.SINGULARITY_LIVE_CONTEXT_PACK;
  if (!contextPack && prompts.promptMenu) {
    const packs = await installedContextPackChoices(env, platform);
    if (packs.length === 0) {
      throw new Error(
        "No installed context packs found in the application data directory",
      );
    }
    contextPack = await prompts.promptMenu("Choose an installed context pack:", packs);
  } else if (!contextPack) {
    contextPack = await prompts.promptText("Installed context pack ID", "");
  }
  const keyName = apiKeyName(provider);
  const providerLabel = providerLabelFor(provider);
  const apiKey =
    env[keyName] ||
    (await prompts.promptSecret(`${providerLabel} API key (input hidden): `));
  const sonioxApiKey =
    env.SONIOX_API_KEY ||
    (await prompts.promptSecret("Soniox API key (optional; press Enter to skip): "));
  return {
    task,
    provider,
    model,
    contextPack,
    apiKey,
    sonioxApiKey,
    timeoutSeconds:
      env.SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS || DEFAULT_TIMEOUT_SECONDS,
  };
}

async function installedContextPackChoices(env, platform) {
  const applicationDataDirectory = applicationDataPath(env, platform);
  let directories;
  try {
    directories = await readdir(join(applicationDataDirectory, "context-packs"), {
      withFileTypes: true,
    });
  } catch (error) {
    if (isMissingPathError(error)) {
      return [];
    }
    throw new Error("Installed context packs could not be listed");
  }

  const packs = [];
  for (const directory of directories) {
    if (!directory.isDirectory() || !/^[A-Za-z0-9_-]{1,64}$/.test(directory.name)) {
      continue;
    }
    try {
      const files = await readdir(
        join(applicationDataDirectory, "context-packs", directory.name),
        { withFileTypes: true },
      );
      if (files.some((file) => file.name === "manifest.yaml" && file.isFile())) {
        packs.push({ value: directory.name, label: directory.name });
      }
    } catch (error) {
      if (!isMissingPathError(error)) {
        throw new Error("Installed context packs could not be listed");
      }
    }
  }
  return packs.sort((left, right) => left.label.localeCompare(right.label));
}

function applicationDataPath(env, platform) {
  const homeDirectory = homedir();
  if (platform === "win32") {
    return join(
      env.APPDATA || join(homeDirectory, "AppData", "Roaming"),
      "local.singularity.live",
    );
  }
  if (platform === "darwin") {
    return join(
      homeDirectory,
      "Library",
      "Application Support",
      "local.singularity.live",
    );
  }
  return join(
    env.XDG_DATA_HOME || join(homeDirectory, ".local", "share"),
    "local.singularity.live",
  );
}

function isMissingPathError(error) {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function parseArgumentValues(argv, env) {
  const options = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    if (option === "--help") {
      throw new Error(
        "Usage: pnpm assist --task <text|screenshot> [--model <model-id>] [--context-pack <id>]",
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
  const model =
    options.get("--model") ||
    env.SINGULARITY_LIVE_MODEL ||
    (env.SINGULARITY_LIVE_PROVIDER === "gemini"
      ? DEFAULT_GEMINI_MODEL
      : env.SINGULARITY_LIVE_PROVIDER === "openai"
        ? DEFAULT_OPENAI_MODEL
        : DEFAULT_MODEL);
  const provider = env.SINGULARITY_LIVE_PROVIDER || inferProvider(model);
  const keyName = apiKeyName(provider);
  const apiKey = env[keyName];
  if (!apiKey || apiKey.trim().length === 0) {
    throw new Error(`Set ${keyName} in the environment before using argument mode`);
  }
  return {
    task,
    provider,
    model,
    contextPack:
      options.get("--context-pack") || env.SINGULARITY_LIVE_CONTEXT_PACK || "",
    apiKey,
    sonioxApiKey: env.SONIOX_API_KEY || "",
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
  if (!["openai", "openrouter", "gemini"].includes(values.provider)) {
    throw new Error("Choose openai, openrouter, or gemini as the configured provider");
  }
  if (
    !/^[A-Za-z0-9][A-Za-z0-9._:/-]{0,159}$/.test(values.model) ||
    looksLikeCredential(values.model)
  ) {
    throw new Error("The provider model ID is invalid");
  }
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(values.contextPack)) {
    throw new Error("Set the ID of an installed context pack");
  }
  if (!/^(?:[5-9]|[1-9][0-9]|[12][0-9]{2}|300)$/.test(values.timeoutSeconds)) {
    throw new Error("Request timeout must be an integer from 5 to 300 seconds");
  }
  if (typeof values.apiKey !== "string" || values.apiKey.trim().length === 0) {
    throw new Error(`Set ${apiKeyName(values.provider)} before launching the app`);
  }
}

function inferProvider(model) {
  if (/^gemini(?:[-/]|$)/i.test(model)) {
    return "gemini";
  }
  if (/^gpt-/i.test(model)) {
    return "openai";
  }
  return "openrouter";
}

function apiKeyName(provider) {
  if (provider === "openai") {
    return "OPENAI_API_KEY";
  }
  return provider === "gemini" ? "GEMINI_API_KEY" : "OPENROUTER_API_KEY";
}

function providerLabelFor(provider) {
  if (provider === "openai") {
    return "OpenAI";
  }
  return provider === "gemini" ? "Google AI Studio" : "OpenRouter";
}

function looksLikeCredential(value) {
  return (
    /^(?:sk-|or-)[A-Za-z0-9_-]{12,}$/i.test(value) ||
    /^AIza[A-Za-z0-9_-]{35}$/.test(value) ||
    /^[A-Za-z0-9_-]{48,}$/.test(value)
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
    promptMenu: promptChoice,
    promptText,
    promptSecret,
    spawn: spawnDevProcess,
    writeError: (message) => process.stderr.write(`${message}\n`),
  });
  process.exitCode = exitCode;
}
