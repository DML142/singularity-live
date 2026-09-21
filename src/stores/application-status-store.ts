import { create } from "zustand";

import {
  getApplicationStatus,
  type ApplicationStatus,
} from "../lib/tauri/application-client";

type BackendConnection =
  | { readonly phase: "loading"; readonly label: "Checking backend" }
  | {
      readonly phase: "ready";
      readonly label: "Backend ready";
      readonly status: ApplicationStatus;
    }
  | { readonly phase: "unavailable"; readonly label: "Backend unavailable" };

interface ApplicationStatusStore {
  readonly backend: BackendConnection;
  readonly loadStatus: () => Promise<void>;
}

export const useApplicationStatusStore = create<ApplicationStatusStore>((set) => ({
  backend: { phase: "loading", label: "Checking backend" },
  loadStatus: async () => {
    set({ backend: { phase: "loading", label: "Checking backend" } });
    try {
      const status = await getApplicationStatus();
      set({ backend: { phase: "ready", label: "Backend ready", status } });
    } catch {
      set({ backend: { phase: "unavailable", label: "Backend unavailable" } });
    }
  },
}));
