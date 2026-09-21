import { invoke } from "@tauri-apps/api/core";

export type BackendState = "ready";

export interface ApplicationStatus {
  readonly applicationName: string;
  readonly version: string;
  readonly backendState: BackendState;
}

export function getApplicationStatus(): Promise<ApplicationStatus> {
  return invoke<ApplicationStatus>("get_app_status");
}
