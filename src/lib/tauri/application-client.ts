import { invoke } from "@tauri-apps/api/core";

export type BackendState = "ready";

export interface ApplicationStatus {
  readonly applicationName: string;
  readonly version: string;
  readonly backendState: BackendState;
}

function isApplicationStatus(value: unknown): value is ApplicationStatus {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  return (
    "applicationName" in value &&
    value.applicationName === "Singularity Live" &&
    "version" in value &&
    typeof value.version === "string" &&
    value.version.length > 0 &&
    "backendState" in value &&
    value.backendState === "ready"
  );
}

export async function getApplicationStatus(): Promise<ApplicationStatus> {
  const response = await invoke<unknown>("get_app_status");

  if (!isApplicationStatus(response)) {
    throw new Error("Invalid application status response");
  }

  return response;
}
