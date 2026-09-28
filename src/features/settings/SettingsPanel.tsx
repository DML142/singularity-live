import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import {
  APP_SCALE_STEP,
  DEFAULT_APP_SCALE,
  DEFAULT_WINDOW_OPACITY,
  MAX_APP_SCALE,
  MIN_APP_SCALE,
  getAppScale,
  getScreenshotPreferences,
  getWindowOpacity,
  setAppScale,
  setScreenshotPreferences,
  setWindowOpacity,
  type ScreenshotPreferences,
} from "../../lib/tauri/customization-client";
import {
  formatShortcutChord,
  getShortcutBindings,
  newShortcutBinding,
  updateShortcutBindings,
  type ShortcutBinding,
  type ShortcutBindingView,
  type ShortcutAction,
  type ShortcutChord,
} from "../../lib/tauri/shortcut-client";
import { useVoiceInputStore } from "../../stores/voice-input-store";
import { useScreenAssistanceStore } from "../../stores/screen-assistance-store";
import {
  addUserContextFiles,
  getUserContextFiles,
  removeUserContextFile,
  type UserContextFileInfo,
} from "../../lib/tauri/user-context-client";
import { recordedShortcutFromEvent } from "./key-chord";

interface SettingsPanelProps {
  readonly onBack: () => void;
}

export function SettingsPanel({ onBack }: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState<
    "binds" | "audio" | "customization" | "context"
  >("binds");
  const [views, setViews] = useState<readonly ShortcutBindingView[]>([]);
  const [drafts, setDrafts] = useState<readonly ShortcutBinding[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [recordingId, setRecordingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void getShortcutBindings()
      .then((loaded) => {
        if (!active) {
          return;
        }
        setViews(loaded);
        setDrafts(loaded.map((view) => view.binding));
      })
      .catch(() => {
        if (active) {
          setError("Shortcut settings are unavailable on this desktop");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
      setRecordingId(null);
    };
  }, []);

  const isDirty = useMemo(
    () => JSON.stringify(drafts) !== JSON.stringify(views.map((view) => view.binding)),
    [drafts, views],
  );

  function updateChord(bindingId: string, chord: ShortcutChord | null): void {
    setDrafts((current) =>
      current.map((binding) =>
        binding.id === bindingId ? { ...binding, chord } : binding,
      ),
    );
    setError(null);
  }

  function updateAction(bindingId: string, action: ShortcutAction): void {
    setDrafts((current) =>
      current.map((binding) =>
        binding.id === bindingId ? { ...binding, action } : binding,
      ),
    );
    setError(null);
  }

  function addBinding(): void {
    setDrafts((current) => [...current, newShortcutBinding()]);
    setError(null);
  }

  function removeBinding(bindingId: string): void {
    if (drafts.length <= 1) {
      return;
    }
    setDrafts((current) => current.filter((binding) => binding.id !== bindingId));
    setRecordingId(null);
    setError(null);
  }

  async function saveBindings(): Promise<void> {
    setSaving(true);
    setError(null);
    setRecordingId(null);
    try {
      const saved = await updateShortcutBindings(drafts);
      setViews(saved);
      setDrafts(saved.map((view) => view.binding));
    } catch (saveError) {
      setError(shortcutSaveErrorMessage(saveError));
    } finally {
      setSaving(false);
    }
  }

  function recordKey(bindingId: string, event: KeyboardEvent<HTMLButtonElement>): void {
    if (recordingId !== bindingId || loading || saving) {
      return;
    }
    const result = recordedShortcutFromEvent(event.nativeEvent);
    if (result.type === "ignore") {
      return;
    }
    event.preventDefault();
    if (result.type === "clear") {
      updateChord(bindingId, null);
      setRecordingId(null);
      return;
    }
    if (result.type === "invalid") {
      setError(result.message);
      return;
    }
    updateChord(bindingId, result.chord);
    setRecordingId(null);
  }

  return (
    <section className="settings-panel" aria-labelledby="settings-heading">
      <header className="settings-heading">
        <div>
          <p className="settings-eyebrow">Settings</p>
          <h2 id="settings-heading">
            {activeTab === "binds"
              ? "Binds"
              : activeTab === "audio"
                ? "Audio input"
                : activeTab === "context"
                  ? "Context files"
                  : "Customization"}
          </h2>
          <p className="settings-description">
            {activeTab === "binds"
              ? "Configure capture, audio-source, click-through, voice-input, and send shortcuts."
              : activeTab === "audio"
                ? "Choose the microphone or system audio source used by voice transcription."
                : activeTab === "context"
                  ? "Choose Markdown and text files that are included with every assistant request."
                  : "Adjust window appearance, app scale, and screenshot capture settings."}
          </p>
        </div>
        <button className="settings-back-button" type="button" onClick={onBack}>
          Back to assistant
        </button>
      </header>

      <div className="settings-tabs" role="tablist" aria-label="Settings sections">
        <button
          className="settings-tab"
          type="button"
          role="tab"
          id="settings-tab-binds"
          aria-controls="settings-panel-binds"
          aria-selected={activeTab === "binds"}
          onClick={() => {
            setActiveTab("binds");
          }}
        >
          Binds
        </button>
        <button
          className="settings-tab"
          type="button"
          role="tab"
          id="settings-tab-audio"
          aria-controls="settings-panel-audio"
          aria-selected={activeTab === "audio"}
          onClick={() => {
            setActiveTab("audio");
          }}
        >
          Audio
        </button>
        <button
          className="settings-tab"
          type="button"
          role="tab"
          id="settings-tab-customization"
          aria-controls="settings-panel-customization"
          aria-selected={activeTab === "customization"}
          onClick={() => {
            setActiveTab("customization");
          }}
        >
          Customization
        </button>
        <button
          className="settings-tab"
          type="button"
          role="tab"
          id="settings-tab-context"
          aria-controls="settings-panel-context"
          aria-selected={activeTab === "context"}
          onClick={() => {
            setActiveTab("context");
          }}
        >
          Context
        </button>
      </div>

      {activeTab === "context" ? (
        <ContextFilesSettings />
      ) : activeTab === "customization" ? (
        <CustomizationSettings />
      ) : activeTab === "audio" ? (
        <AudioSettings />
      ) : loading ? (
        <p className="settings-status" role="status">
          Loading shortcut bindings…
        </p>
      ) : (
        <div
          id="settings-panel-binds"
          role="tabpanel"
          aria-labelledby="settings-tab-binds"
        >
          {error !== null && (
            <p className="settings-error" role="alert">
              {error}
            </p>
          )}
          <div className="shortcut-bindings">
            {drafts.map((binding, index) => {
              const view = views.find((entry) => entry.binding.id === binding.id);
              const changed =
                view !== undefined &&
                JSON.stringify(view.binding) !== JSON.stringify(binding);
              const registration = view?.registration;
              return (
                <article
                  className="shortcut-binding-row"
                  data-testid="shortcut-binding-row"
                  key={binding.id}
                >
                  <div className="shortcut-binding-title">
                    <span>Binding {index + 1}</span>
                    <span
                      className="shortcut-registration"
                      data-state={
                        changed ? "unsaved" : (registration?.status ?? "unbound")
                      }
                    >
                      {changed
                        ? "Unsaved"
                        : registration?.status === "registered"
                          ? `Active · ${registration.effectiveTrigger}`
                          : registration?.status === "failed"
                            ? registration.message
                            : "Not registered"}
                    </span>
                  </div>
                  <label className="shortcut-action-label">
                    Action
                    <select
                      aria-label="Action"
                      value={binding.action}
                      disabled={saving}
                      onChange={(event) => {
                        if (
                          event.currentTarget.value === "screenshot" ||
                          event.currentTarget.value === "screenshot_send" ||
                          event.currentTarget.value === "toggle_taskbar_icon" ||
                          event.currentTarget.value === "voice_input" ||
                          event.currentTarget.value === "toggle_audio_source" ||
                          event.currentTarget.value === "toggle_click_through" ||
                          event.currentTarget.value === "quick_send" ||
                          event.currentTarget.value === "min_mode"
                        ) {
                          updateAction(binding.id, event.currentTarget.value);
                        }
                      }}
                    >
                      <option value="screenshot">Screenshot</option>
                      <option value="screenshot_send">Send screenshot</option>
                      <option value="toggle_taskbar_icon">
                        Hide / show taskbar icon
                      </option>
                      <option value="voice_input">
                        Voice input · toggle recording
                      </option>
                      <option value="toggle_audio_source">
                        Swap microphone / system audio
                      </option>
                      <option value="toggle_click_through">
                        Toggle mouse click-through
                      </option>
                      <option value="quick_send">Send current message</option>
                      <option value="min_mode">Toggle minmode</option>
                    </select>
                  </label>
                  <div className="shortcut-chord-controls">
                    <button
                      className="shortcut-record-button"
                      type="button"
                      aria-label="Record shortcut"
                      aria-pressed={recordingId === binding.id}
                      disabled={saving}
                      onClick={() => {
                        setError(null);
                        setRecordingId((current) =>
                          current === binding.id ? null : binding.id,
                        );
                      }}
                      onKeyDown={(event) => {
                        recordKey(binding.id, event);
                      }}
                      onBlur={() => {
                        if (recordingId === binding.id) {
                          setRecordingId(null);
                        }
                      }}
                    >
                      {recordingId === binding.id
                        ? "Press a shortcut · Esc clears"
                        : formatShortcutChord(binding.chord)}
                    </button>
                    <button
                      className="shortcut-clear-button"
                      type="button"
                      onClick={() => {
                        updateChord(binding.id, null);
                      }}
                      disabled={saving || binding.chord === null}
                    >
                      Clear
                    </button>
                    <button
                      className="shortcut-remove-button"
                      type="button"
                      aria-label="Remove binding"
                      onClick={() => {
                        removeBinding(binding.id);
                      }}
                      disabled={saving || drafts.length <= 1}
                    >
                      Remove
                    </button>
                  </div>
                  {recordingId === binding.id && (
                    <p className="shortcut-recording-help">
                      Press one key, or hold up to two modifiers and press another key.
                    </p>
                  )}
                </article>
              );
            })}
          </div>
          <footer className="settings-footer">
            <button
              className="settings-add-button"
              type="button"
              onClick={addBinding}
              disabled={saving}
            >
              Add binding
            </button>
            <button
              className="settings-save-button"
              type="button"
              onClick={() => void saveBindings()}
              disabled={saving || !isDirty || drafts.length === 0}
            >
              {saving ? "Saving…" : "Save bindings"}
            </button>
          </footer>
        </div>
      )}
    </section>
  );
}

function AudioSettings() {
  const source = useVoiceInputStore((state) => state.source);
  const devices = useVoiceInputStore((state) => state.devices);
  const microphoneDeviceId = useVoiceInputStore((state) => state.microphoneDeviceId);
  const phase = useVoiceInputStore((state) => state.phase);
  const error = useVoiceInputStore((state) => state.error);
  const setSource = useVoiceInputStore((state) => state.setSource);
  const loadDevices = useVoiceInputStore((state) => state.loadDevices);
  const recording =
    phase === "connecting" || phase === "recording" || phase === "stopping";

  return (
    <div
      id="settings-panel-audio"
      className="customization-panel"
      role="tabpanel"
      aria-labelledby="settings-tab-audio"
    >
      <section className="customization-card" aria-labelledby="audio-input-heading">
        <div>
          <h3 id="audio-input-heading">Voice input</h3>
          <p className="settings-description">
            The selected source is transcribed to text. Recording starts and stops from
            the chat composer.
          </p>
        </div>
        <label className="audio-device-control">
          Source
          <select
            aria-label="Audio source"
            value={source}
            disabled={recording}
            onChange={(event) => {
              if (
                event.currentTarget.value === "microphone" ||
                event.currentTarget.value === "system_audio"
              ) {
                void setSource(event.currentTarget.value, microphoneDeviceId);
              }
            }}
          >
            <option value="microphone">Microphone</option>
            <option value="system_audio">System audio</option>
          </select>
        </label>
        {source === "microphone" ? (
          <label className="audio-device-control">
            Microphone
            <select
              aria-label="Microphone device"
              value={microphoneDeviceId ?? ""}
              disabled={recording}
              onChange={(event) => {
                void setSource(
                  "microphone",
                  event.currentTarget.value.length > 0
                    ? event.currentTarget.value
                    : null,
                );
              }}
            >
              <option value="">Default microphone</option>
              {devices.map((device) => (
                <option key={device.id} value={device.id}>
                  {device.label}
                  {device.isDefault ? " · default" : ""}
                </option>
              ))}
            </select>
            <button
              className="settings-add-button"
              type="button"
              onClick={() => void loadDevices()}
              disabled={recording}
            >
              Refresh devices
            </button>
          </label>
        ) : null}
        <p className="settings-status" role="status">
          Device selection is saved automatically.
        </p>
        {error !== null ? (
          <p className="settings-error" role="alert">
            {error}
          </p>
        ) : null}
      </section>
    </div>
  );
}

function CustomizationSettings() {
  const [appScale, setAppScaleState] = useState(DEFAULT_APP_SCALE);
  const [savedAppScale, setSavedAppScale] = useState(DEFAULT_APP_SCALE);
  const [loadingAppScale, setLoadingAppScale] = useState(true);
  const [savingAppScale, setSavingAppScale] = useState(false);
  const [appScaleError, setAppScaleError] = useState<string | null>(null);
  const [opacity, setOpacity] = useState(DEFAULT_WINDOW_OPACITY);
  const [savedOpacity, setSavedOpacity] = useState(DEFAULT_WINDOW_OPACITY);
  const [loadingOpacity, setLoadingOpacity] = useState(true);
  const [savingOpacity, setSavingOpacity] = useState(false);
  const [opacityError, setOpacityError] = useState<string | null>(null);
  const [screenshotPreferences, setScreenshotPreferencesState] =
    useState<ScreenshotPreferences>({
      closeWindowOnScreenshot: false,
      targetKind: "monitor",
    });
  const [loadingScreenshotPreferences, setLoadingScreenshotPreferences] =
    useState(true);
  const [savingScreenshotPreferences, setSavingScreenshotPreferences] = useState(false);
  const [screenshotPreferencesError, setScreenshotPreferencesError] = useState<
    string | null
  >(null);
  const savedOpacityRef = useRef(DEFAULT_WINDOW_OPACITY);
  const setScreenshotTargetKind = useScreenAssistanceStore(
    (state) => state.setTargetKind,
  );

  useEffect(() => {
    let active = true;
    void getAppScale()
      .then((value) => {
        if (active) {
          setAppScaleState(value);
          setSavedAppScale(value);
        }
      })
      .catch(() => {
        if (active) {
          setAppScaleError("Application scale settings could not be loaded");
        }
      })
      .finally(() => {
        if (active) {
          setLoadingAppScale(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void getWindowOpacity()
      .then((value) => {
        if (active) {
          setOpacity(value);
          setSavedOpacity(value);
          savedOpacityRef.current = value;
          document.documentElement.dataset.appOpacity = String(value);
        }
      })
      .catch(() => {
        if (active) {
          setOpacityError("Window appearance settings could not be loaded");
        }
      })
      .finally(() => {
        if (active) {
          setLoadingOpacity(false);
        }
      });
    return () => {
      active = false;
      document.documentElement.dataset.appOpacity = String(savedOpacityRef.current);
    };
  }, []);

  useEffect(() => {
    let active = true;
    void getScreenshotPreferences()
      .then((preferences) => {
        if (active) {
          setScreenshotPreferencesState(preferences);
          setScreenshotTargetKind(preferences.targetKind);
        }
      })
      .catch(() => {
        if (active) {
          setScreenshotPreferencesError("Screenshot settings could not be loaded");
        }
      })
      .finally(() => {
        if (active) {
          setLoadingScreenshotPreferences(false);
        }
      });
    return () => {
      active = false;
    };
  }, [setScreenshotTargetKind]);

  async function updateScreenshotPreferences(
    preferences: ScreenshotPreferences,
  ): Promise<void> {
    const previous = screenshotPreferences;
    setScreenshotPreferencesState(preferences);
    setScreenshotPreferencesError(null);
    setSavingScreenshotPreferences(true);
    try {
      const saved = await setScreenshotPreferences(preferences);
      setScreenshotPreferencesState(saved);
      setScreenshotTargetKind(saved.targetKind);
    } catch {
      setScreenshotPreferencesState(previous);
      setScreenshotPreferencesError("Screenshot settings could not be saved");
    } finally {
      setSavingScreenshotPreferences(false);
    }
  }

  async function saveOpacity(): Promise<void> {
    setSavingOpacity(true);
    setOpacityError(null);
    try {
      const saved = await setWindowOpacity(opacity);
      setOpacity(saved);
      setSavedOpacity(saved);
      savedOpacityRef.current = saved;
      document.documentElement.dataset.appOpacity = String(saved);
    } catch {
      setOpacityError("Window appearance settings could not be saved");
      document.documentElement.dataset.appOpacity = String(savedOpacityRef.current);
      setOpacity(savedOpacityRef.current);
    } finally {
      setSavingOpacity(false);
    }
  }

  async function saveAppScale(): Promise<void> {
    setSavingAppScale(true);
    setAppScaleError(null);
    try {
      const saved = await setAppScale(appScale);
      setAppScaleState(saved);
      setSavedAppScale(saved);
    } catch {
      setAppScaleError("Application scale settings could not be saved");
      setAppScaleState(savedAppScale);
    } finally {
      setSavingAppScale(false);
    }
  }

  return (
    <div
      id="settings-panel-customization"
      className="customization-panel"
      role="tabpanel"
      aria-labelledby="settings-tab-customization"
    >
      <section className="customization-card" aria-labelledby="opacity-heading">
        <div>
          <h3 id="opacity-heading">Window opacity</h3>
          <p className="settings-description">
            Lower opacity lets you see more of the desktop behind the assistant.
          </p>
        </div>
        <label className="opacity-control" htmlFor="window-opacity">
          <span>Opacity</span>
          <output htmlFor="window-opacity">{opacity}%</output>
          <input
            id="window-opacity"
            type="range"
            min="40"
            max="100"
            step="5"
            value={opacity}
            disabled={loadingOpacity || savingOpacity}
            onChange={(event) => {
              const next = Number(event.currentTarget.value);
              setOpacity(next);
              document.documentElement.dataset.appOpacity = String(next);
            }}
          />
          <span className="opacity-range-labels">
            <span>More transparent</span>
            <span>More opaque</span>
          </span>
        </label>
        {opacityError !== null ? (
          <p className="settings-error" role="alert">
            {opacityError}
          </p>
        ) : null}
        <footer className="settings-footer">
          <button
            className="settings-save-button"
            type="button"
            onClick={() => void saveOpacity()}
            disabled={loadingOpacity || savingOpacity || opacity === savedOpacity}
          >
            {savingOpacity ? "Saving…" : "Save appearance"}
          </button>
        </footer>
      </section>
      <section className="customization-card" aria-labelledby="app-scale-heading">
        <div>
          <h3 id="app-scale-heading">Application scale</h3>
          <p className="settings-description">
            Change the size of the app content, like browser zoom.
          </p>
        </div>
        <label className="opacity-control" htmlFor="app-scale">
          <span>Scale</span>
          <output htmlFor="app-scale">{appScale}%</output>
          <input
            id="app-scale"
            type="range"
            min={MIN_APP_SCALE}
            max={MAX_APP_SCALE}
            step={APP_SCALE_STEP}
            value={appScale}
            disabled={loadingAppScale || savingAppScale}
            onChange={(event) => {
              setAppScaleState(Number(event.currentTarget.value));
            }}
          />
          <span className="opacity-range-labels">
            <span>Smaller</span>
            <span>Larger</span>
          </span>
        </label>
        {appScaleError !== null ? (
          <p className="settings-error" role="alert">
            {appScaleError}
          </p>
        ) : null}
        <footer className="settings-footer">
          <button
            className="settings-save-button"
            type="button"
            onClick={() => void saveAppScale()}
            disabled={loadingAppScale || savingAppScale || appScale === savedAppScale}
          >
            {savingAppScale ? "Saving…" : "Save scale"}
          </button>
        </footer>
      </section>
      <section className="customization-card" aria-labelledby="screenshot-heading">
        <div>
          <h3 id="screenshot-heading">Screenshot capture</h3>
          <p className="settings-description">
            Choose the source used by the screenshot bind and chat capture button.
          </p>
        </div>
        <label className="audio-device-control">
          Capture source
          <select
            aria-label="Screenshot capture source"
            value={screenshotPreferences.targetKind}
            disabled={loadingScreenshotPreferences || savingScreenshotPreferences}
            onChange={(event) => {
              if (
                event.currentTarget.value === "monitor" ||
                event.currentTarget.value === "window"
              ) {
                void updateScreenshotPreferences({
                  ...screenshotPreferences,
                  targetKind: event.currentTarget.value,
                });
              }
            }}
          >
            <option value="monitor">Entire screen</option>
            <option value="window">Window under pointer</option>
          </select>
        </label>
        <label className="screenshot-close-control">
          <input
            type="checkbox"
            checked={screenshotPreferences.closeWindowOnScreenshot}
            disabled={loadingScreenshotPreferences || savingScreenshotPreferences}
            onChange={(event) => {
              void updateScreenshotPreferences({
                ...screenshotPreferences,
                closeWindowOnScreenshot: event.currentTarget.checked,
              });
            }}
          />
          <span>Close window on screenshot</span>
        </label>
        <p className="settings-status" role="status">
          {savingScreenshotPreferences
            ? "Saving screenshot settings…"
            : "Screenshot settings are saved automatically."}
        </p>
        {screenshotPreferencesError !== null ? (
          <p className="settings-error" role="alert">
            {screenshotPreferencesError}
          </p>
        ) : null}
      </section>
    </div>
  );
}

function ContextFilesSettings() {
  const [files, setFiles] = useState<readonly UserContextFileInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void getUserContextFiles()
      .then((result) => {
        if (active) {
          setFiles(result);
        }
      })
      .catch(() => {
        if (active) {
          setError("Context files could not be loaded");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  async function addFiles(): Promise<void> {
    setSaving(true);
    setError(null);
    try {
      setFiles(await addUserContextFiles());
    } catch (saveError) {
      setError(
        typeof saveError === "string" ? saveError : "Context files could not be added",
      );
    } finally {
      setSaving(false);
    }
  }

  async function removeFile(file: UserContextFileInfo): Promise<void> {
    setSaving(true);
    setError(null);
    try {
      setFiles(await removeUserContextFile(file.id));
    } catch {
      setError(`“${file.name}” could not be removed`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      id="settings-panel-context"
      className="customization-panel"
      role="tabpanel"
      aria-labelledby="settings-tab-context"
    >
      <section className="customization-card" aria-labelledby="context-files-heading">
        <div>
          <h3 id="context-files-heading">Always included context</h3>
          <p className="settings-description">
            Add .md or .txt files to guide every assistant request. Files are stored in
            the app and included with each text or screenshot request, up to 8 files and
            12 KiB total.
          </p>
        </div>
        <button
          className="settings-save-button"
          type="button"
          onClick={() => void addFiles()}
          disabled={loading || saving}
        >
          {saving ? "Saving…" : "Add context files"}
        </button>
        {loading ? (
          <p className="settings-status" role="status">
            Loading context files…
          </p>
        ) : files.length === 0 ? (
          <p className="settings-status" role="status">
            No context files added.
          </p>
        ) : (
          <ul className="context-file-list" aria-label="Added context files">
            {files.map((file) => (
              <li key={file.id} className="context-file-row">
                <span>{file.name}</span>
                <button
                  className="settings-save-button"
                  type="button"
                  onClick={() => void removeFile(file)}
                  disabled={saving}
                  aria-label={`Remove ${file.name}`}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        {error !== null ? (
          <p className="settings-error" role="alert">
            {error}
          </p>
        ) : null}
      </section>
    </div>
  );
}

function shortcutSaveErrorMessage(error: unknown): string {
  const commandError = parseShortcutCommandError(error);
  if (commandError !== null && commandError.code === "registrarUnavailable") {
    return "Global shortcut registration is unavailable in this desktop session. Try an X11 session or a Wayland desktop with GlobalShortcuts support. The saved bindings are unchanged.";
  }
  if (commandError !== null && commandError.code === "registrationRejected") {
    const failures = commandError.failures;
    if (isUnknownArray(failures)) {
      const messages = failures.flatMap((failure) => {
        if (
          isRecord(failure) &&
          typeof failure.message === "string" &&
          failure.message.trim().length > 0
        ) {
          return [failure.message];
        }
        return [];
      });
      if (messages.length > 0) {
        return `${messages.join("; ")} The saved bindings are unchanged.`;
      }
    }
    if (
      typeof commandError.message === "string" &&
      commandError.message.trim().length > 0
    ) {
      return `${commandError.message} The saved bindings are unchanged.`;
    }
  }
  if (
    commandError !== null &&
    typeof commandError.message === "string" &&
    commandError.message.trim().length > 0
  ) {
    return `${commandError.message} The saved bindings are unchanged.`;
  }
  const rawMessage =
    typeof error === "string"
      ? error
      : isRecord(error) && typeof error.message === "string"
        ? error.message
        : null;
  if (rawMessage !== null && rawMessage.trim().length > 0) {
    return `${rawMessage} The saved bindings are unchanged.`;
  }
  return "One or more shortcuts could not be registered. The saved bindings are unchanged.";
}

function parseShortcutCommandError(error: unknown): Record<string, unknown> | null {
  if (isRecord(error) && "code" in error) {
    return error;
  }
  const message =
    isRecord(error) && typeof error.message === "string" ? error.message : error;
  if (typeof message !== "string") {
    return null;
  }
  try {
    const parsed: unknown = JSON.parse(message);
    return isRecord(parsed) ? parsed : null;
  } catch {
    return message.trim().length > 0 ? { message } : null;
  }
}

function isUnknownArray(value: unknown): value is unknown[] {
  return Array.isArray(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
