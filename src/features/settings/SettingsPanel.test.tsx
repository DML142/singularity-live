import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SettingsPanel } from "./SettingsPanel";
import { useVoiceInputStore } from "../../stores/voice-input-store";
import type {
  ShortcutBinding,
  ShortcutBindingView,
} from "../../lib/tauri/shortcut-client";

const { getShortcutBindingsMock, updateShortcutBindingsMock } = vi.hoisted(() => ({
  getShortcutBindingsMock: vi.fn<() => Promise<readonly ShortcutBindingView[]>>(),
  updateShortcutBindingsMock:
    vi.fn<
      (bindings: readonly ShortcutBinding[]) => Promise<readonly ShortcutBindingView[]>
    >(),
}));
const { getWindowOpacityMock, setWindowOpacityMock } = vi.hoisted(() => ({
  getWindowOpacityMock: vi.fn<() => Promise<number>>(),
  setWindowOpacityMock: vi.fn<(percentage: number) => Promise<number>>(),
}));

vi.mock("../../lib/tauri/shortcut-client", async () => {
  const actual = await vi.importActual<
    typeof import("../../lib/tauri/shortcut-client")
  >("../../lib/tauri/shortcut-client");
  return {
    ...actual,
    getShortcutBindings: getShortcutBindingsMock,
    updateShortcutBindings: updateShortcutBindingsMock,
  };
});
vi.mock("../../lib/tauri/customization-client", () => ({
  DEFAULT_WINDOW_OPACITY: 100,
  getWindowOpacity: getWindowOpacityMock,
  setWindowOpacity: setWindowOpacityMock,
}));

const initialView: ShortcutBindingView = {
  binding: {
    id: "00000000-0000-4000-8000-000000000001",
    action: "screenshot" as const,
    chord: { modifiers: ["control", "super"] as const, key: "KeyP" },
  },
  registration: {
    status: "registered" as const,
    effectiveTrigger: "Ctrl+Super+P",
  },
};

describe("Binds settings", () => {
  beforeEach(() => {
    getShortcutBindingsMock.mockReset().mockResolvedValue([initialView]);
    getWindowOpacityMock.mockReset().mockResolvedValue(100);
    setWindowOpacityMock
      .mockReset()
      .mockImplementation((value) => Promise.resolve(value));
    delete document.documentElement.dataset.appOpacity;
    updateShortcutBindingsMock.mockReset().mockImplementation((bindings) =>
      Promise.resolve(
        bindings.map((binding) => ({
          binding,
          registration:
            binding.chord === null
              ? { status: "unbound" as const }
              : { status: "registered" as const, effectiveTrigger: "Ctrl+Super+P" },
        })),
      ),
    );
  });

  it("adds and removes arbitrary rows while retaining one", async () => {
    render(<SettingsPanel onBack={() => {}} />);

    expect(await screen.findAllByTestId("shortcut-binding-row")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Add binding" }));
    expect(screen.getAllByTestId("shortcut-binding-row")).toHaveLength(2);

    const removeButtons = screen.getAllByRole("button", { name: "Remove binding" });
    const firstRemoveButton = removeButtons.at(0);
    expect(firstRemoveButton).toBeDefined();
    if (firstRemoveButton === undefined) {
      throw new Error("The first binding remove control is missing");
    }
    fireEvent.click(firstRemoveButton);
    expect(screen.getAllByTestId("shortcut-binding-row")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Remove binding" })).toBeDisabled();
  });

  it("records only while armed and Escape clears the row", async () => {
    render(<SettingsPanel onBack={() => {}} />);

    const row = await screen.findByTestId("shortcut-binding-row");
    const recordButton = within(row).getByRole("button", { name: "Record shortcut" });
    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "KeyQ", ctrlKey: true });
    expect(recordButton).toHaveTextContent("Ctrl + Q");

    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "Escape" });
    expect(recordButton).toHaveTextContent("Not set");
  });

  it("explains when the desktop session does not support global shortcuts", async () => {
    updateShortcutBindingsMock.mockRejectedValue({ code: "registrarUnavailable" });
    render(<SettingsPanel onBack={() => {}} />);

    const row = await screen.findByTestId("shortcut-binding-row");
    const recordButton = within(row).getByRole("button", { name: "Record shortcut" });
    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, { code: "KeyQ", ctrlKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Save bindings" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Global shortcut registration is unavailable in this desktop session",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Try an X11 session or a Wayland desktop with GlobalShortcuts support",
    );
  });

  it("lets users bind quick send and voice recording actions", async () => {
    render(<SettingsPanel onBack={() => {}} />);

    const row = await screen.findByTestId("shortcut-binding-row");
    const action = within(row).getByRole("combobox", { name: "Action" });
    expect(
      within(action).getByRole("option", { name: "Send current message" }),
    ).toBeInTheDocument();
    expect(
      within(action).getByRole("option", { name: "Voice input · toggle recording" }),
    ).toBeInTheDocument();
  });

  it("previews and saves the window opacity setting", async () => {
    render(<SettingsPanel onBack={() => {}} />);
    fireEvent.click(screen.getByRole("tab", { name: "Customization" }));

    const slider = await screen.findByRole("slider", { name: /Opacity/ });
    fireEvent.change(slider, { target: { value: "75" } });
    expect(document.documentElement).toHaveAttribute("data-app-opacity", "75");

    fireEvent.click(screen.getByRole("button", { name: "Save appearance" }));
    await waitFor(() => {
      expect(setWindowOpacityMock).toHaveBeenCalledWith(75);
    });
    expect(document.documentElement).toHaveAttribute("data-app-opacity", "75");
  });

  it("shows microphone and system audio selection in the audio settings tab", async () => {
    useVoiceInputStore.setState({
      source: "microphone",
      devices: [{ id: "usb-mic", label: "USB microphone", isDefault: true }],
      microphoneDeviceId: null,
    });
    render(<SettingsPanel onBack={() => {}} />);
    fireEvent.click(screen.getByRole("tab", { name: "Audio" }));

    expect(await screen.findByRole("combobox", { name: "Audio source" })).toHaveValue(
      "microphone",
    );
    expect(screen.getByRole("combobox", { name: "Microphone device" })).toHaveValue("");
    expect(
      screen.getByRole("option", { name: "USB microphone · default" }),
    ).toBeInTheDocument();
  });
});
