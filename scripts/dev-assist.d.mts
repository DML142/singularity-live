export interface DevAssistChoice {
  readonly value: string;
  readonly label: string;
}

export interface DevAssistSpawnOptions {
  readonly env: Record<string, string | undefined>;
  readonly stdio: "inherit";
}

export interface DevAssistInput {
  readonly isTTY?: boolean;
  setRawMode?: (enabled: boolean) => unknown;
  setEncoding(encoding: string): unknown;
  resume(): unknown;
  pause(): unknown;
  on(event: "data", listener: (chunk: unknown) => void): this;
  on(event: "error", listener: (error: unknown) => void): this;
  off(event: "data", listener: (chunk: unknown) => void): this;
  off(event: "error", listener: (error: unknown) => void): this;
}

export interface DevAssistOutput {
  write(chunk: string): unknown;
}

export interface DevAssistOptions {
  readonly argv: readonly string[];
  readonly env: Record<string, string | undefined>;
  readonly platform?: string;
  readonly promptChoice: (
    prompt: string,
    choices: readonly DevAssistChoice[],
  ) => Promise<string>;
  readonly promptMenu?: (
    prompt: string,
    choices: readonly DevAssistChoice[],
  ) => Promise<string>;
  readonly promptText: (prompt: string, defaultValue: string) => Promise<string>;
  readonly promptSecret: (prompt: string) => Promise<string>;
  readonly spawn: (
    command: string,
    args: readonly string[],
    options: DevAssistSpawnOptions,
  ) => Promise<number>;
  readonly writeError: (message: string) => void;
}

export function launchDevAssist(options: DevAssistOptions): Promise<number>;
export function readHiddenInput(
  input: DevAssistInput,
  output: DevAssistOutput,
  prompt: string,
): Promise<string>;
