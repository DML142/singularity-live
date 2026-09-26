import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import {
  formatShortcutChord,
  getShortcutBindings,
  newShortcutBinding,
  updateShortcutBindings,
  type ShortcutBinding,
  type ShortcutBindingView,
  type ShortcutChord,
} from "../../lib/tauri/shortcut-client";
import { recordedShortcutFromEvent } from "./key-chord";

interface SettingsPanelProps {
  readonly onBack: () => void;
}

export function SettingsPanel({ onBack }: SettingsPanelProps) {
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
    } catch {
      setError(
        "One or more shortcuts could not be registered. The saved bindings are unchanged.",
      );
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
          <h2 id="settings-heading">Binds</h2>
          <p className="settings-description">
            Configure global shortcuts for explicit screenshot capture. Changes take
            effect after saving.
          </p>
        </div>
        <button className="settings-back-button" type="button" onClick={onBack}>
          Back to assistant
        </button>
      </header>

      <div className="settings-tabs" role="tablist" aria-label="Settings sections">
        <button className="settings-tab" type="button" role="tab" aria-selected="true">
          Binds
        </button>
      </div>

      {loading ? (
        <p className="settings-status" role="status">
          Loading shortcut bindings…
        </p>
      ) : (
        <>
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
                    <select aria-label="Action" value="screenshot" disabled>
                      <option value="screenshot">Screenshot</option>
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
        </>
      )}
    </section>
  );
}
