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
const {
  getWindowOpacityMock,
  setWindowOpacityMock,
  getAppScaleMock,
  setAppScaleMock,
  getScreenshotPreferencesMock,
  setScreenshotPreferencesMock,
} = vi.hoisted(() => ({
  getWindowOpacityMock: vi.fn<() => Promise<number>>(),
  setWindowOpacityMock: vi.fn<(percentage: number) => Promise<number>>(),
  getAppScaleMock: vi.fn<() => Promise<number>>(),
  setAppScaleMock: vi.fn<(percentage: number) => Promise<number>>(),
  getScreenshotPreferencesMock: vi.fn<
    () => Promise<{
      readonly closeWindowOnScreenshot: boolean;
      readonly targetKind: "monitor" | "window";
    }>
  >(),
  setScreenshotPreferencesMock: vi.fn<
    (preferences: {
      readonly closeWindowOnScreenshot: boolean;
      readonly targetKind: "monitor" | "window";
    }) => Promise<{
      readonly closeWindowOnScreenshot: boolean;
      readonly targetKind: "monitor" | "window";
    }>
  >(),
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
  DEFAULT_APP_SCALE: 100,
  DEFAULT_WINDOW_OPACITY: 100,
  APP_SCALE_STEP: 10,
  MIN_APP_SCALE: 70,
  MAX_APP_SCALE: 130,
  getAppScale: getAppScaleMock,
  setAppScale: setAppScaleMock,
  getWindowOpacity: getWindowOpacityMock,
  setWindowOpacity: setWindowOpacityMock,
  getScreenshotPreferences: getScreenshotPreferencesMock,
  setScreenshotPreferences: setScreenshotPreferencesMock,
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
    getAppScaleMock.mockReset().mockResolvedValue(100);
    getScreenshotPreferencesMock.mockReset().mockResolvedValue({
      closeWindowOnScreenshot: false,
      targetKind: "monitor",
    });
    setWindowOpacityMock
      .mockReset()
      .mockImplementation((value) => Promise.resolve(value));
    setAppScaleMock.mockReset().mockImplementation((value) => Promise.resolve(value));
    setScreenshotPreferencesMock
      .mockReset()
      .mockImplementation((preferences) => Promise.resolve(preferences));
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
    expect(
      within(action).getByRole("option", {
        name: "Swap microphone / system audio",
      }),
    ).toBeInTheDocument();
    expect(
      within(action).getByRole("option", { name: "Toggle mouse click-through" }),
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

  it("saves application zoom in ten percent steps", async () => {
    render(<SettingsPanel onBack={() => {}} />);
    fireEvent.click(screen.getByRole("tab", { name: "Customization" }));

    const slider = await screen.findByRole("slider", { name: /Scale/ });
    expect(slider).toHaveAttribute("min", "70");
    expect(slider).toHaveAttribute("max", "130");
    expect(slider).toHaveAttribute("step", "10");
    fireEvent.change(slider, { target: { value: "110" } });
    fireEvent.click(screen.getByRole("button", { name: "Save scale" }));

    await waitFor(() => {
      expect(setAppScaleMock).toHaveBeenCalledWith(110);
    });
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
