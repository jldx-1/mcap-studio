import { ArrowRight, CheckCircle2, GitBranch, Layers3 } from "lucide-react";
import type { Recording, Workflow } from "../../types/models";
import { filename } from "../../utils/recordings";

export function Pipeline({
  recording,
  workflow,
  onChange,
  busy,
  onSave,
  onRun,
}: {
  recording: Recording;
  workflow: Workflow;
  onChange: (value: Workflow) => void;
  busy: boolean;
  onSave: () => void;
  onRun: () => void;
}) {
  const selected = workflow.selected_channels;
  const sync = workflow.sync;
  const updateSync = (value: Partial<Workflow["sync"]>) =>
    onChange({ ...workflow, sync: { ...sync, ...value } });
  return (
    <div className="pipeline-layout">
      <div>
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>
                <span className="heading-step">1</span> Select your channels
              </h2>
              <p>Choose the streams to include in the output recording.</p>
            </div>
            <button
              className="text-button"
              onClick={() =>
                onChange({
                  ...workflow,
                  selected_channels:
                    selected.length === recording.topics.length
                      ? []
                      : recording.topics.map((topic) =>
                        String(topic.channel_id),
                      ),
                })
              }
            >
              {selected.length === recording.topics.length
                ? "Clear all"
                : "Select all"}
            </button>
          </div>
          <div className="pipeline-topics">
            {recording.topics.map((topic) => {
              const id = String(topic.channel_id);
              return (
                <div
                  className={`pipeline-topic ${selected.includes(id) ? "included" : ""}`}
                  key={id}
                >
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={selected.includes(id)}
                      onChange={(event) =>
                        onChange({
                          ...workflow,
                          selected_channels: event.target.checked
                            ? [...selected, id]
                            : selected.filter((value) => value !== id),
                        })
                      }
                    />
                    <div>
                      <strong>{topic.topic}</strong>
                      <span>
                        {topic.encoding} · {topic.frame_count.toLocaleString()}{" "}
                        messages
                      </span>
                    </div>
                  </label>
                  <label className="translation-label">
                    <span>Field mapping · optional</span>
                    <textarea
                      aria-label={`Translation for ${topic.topic}`}
                      disabled={
                        !selected.includes(id) || topic.kind === "image"
                      }
                      rows={2}
                      placeholder="* -> *"
                      value={workflow.translations[id] || ""}
                      onChange={(event) =>
                        onChange({
                          ...workflow,
                          translations: {
                            ...workflow.translations,
                            [id]: event.target.value,
                          },
                        })
                      }
                    />
                    <span className="field-help">
                      {"e.g. position.x -> x * 1000"}
                    </span>
                  </label>
                </div>
              );
            })}
          </div>
        </section>
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>
                <span className="heading-step">2</span> Synchronize streams
              </h2>
              <p>Align selected channels to a parent channel’s timestamps.</p>
            </div>
            <label className="toggle" title="Enable synchronization">
              <input
                aria-label="Enable synchronization"
                type="checkbox"
                checked={sync.enabled}
                onChange={(event) =>
                  updateSync({
                    enabled: event.target.checked,
                    parent_channel: sync.parent_channel || selected[0] || "",
                  })
                }
              />
              <span />
            </label>
          </div>
          <div
            className={`sync-fields ${sync.enabled ? "" : "disabled-section"}`}
          >
            <div className="form-grid">
              <label>
                Parent channel
                <select
                  disabled={!sync.enabled}
                  aria-label="Parent channel"
                  value={sync.parent_channel}
                  onChange={(event) =>
                    updateSync({ parent_channel: event.target.value })
                  }
                >
                  <option value="">Choose a channel</option>
                  {recording.topics
                    .filter((topic) =>
                      selected.includes(String(topic.channel_id)),
                    )
                    .map((topic) => (
                      <option key={topic.channel_id} value={topic.channel_id}>
                        {topic.topic}
                      </option>
                    ))}
                </select>
              </label>
              <label>
                Alignment method
                <select
                  disabled={!sync.enabled}
                  value={sync.mode}
                  onChange={(event) => updateSync({ mode: event.target.value })}
                >
                  <option value="basic">Basic · nearest timestamp</option>
                  <option value="nearest">Nearest timestamp</option>
                  <option value="exact">Exact timestamp</option>
                  <option value="linear">Linear interpolation</option>
                  <option value="rate">Rate alignment</option>
                  <option value="interpolate">
                    Interpolate numeric fields
                  </option>
                </select>
              </label>
            </div>
            <label>
              Tolerance (milliseconds)
              <input
                disabled={!sync.enabled}
                type="number"
                min="0"
                step="0.001"
                placeholder="Unlimited"
                value={
                  sync.tolerance_ns === null
                    ? ""
                    : sync.tolerance_ns / 1_000_000
                }
                onChange={(event) =>
                  updateSync({
                    tolerance_ns:
                      event.target.value === ""
                        ? null
                        : Number(event.target.value) * 1_000_000,
                  })
                }
              />
              <span className="field-help">
                Leave blank for unlimited tolerance. Linear interpolation
                supports numeric JSON fields.
              </span>
            </label>
            <label className="check-label">
              <input
                disabled={!sync.enabled}
                type="checkbox"
                checked={sync.delete_out_of_sync}
                onChange={(event) =>
                  updateSync({ delete_out_of_sync: event.target.checked })
                }
              />
              <span>Drop messages outside the synchronization window</span>
            </label>
          </div>
        </section>
      </div>
      <aside className="panel pipeline-summary">
        <span className="summary-icon">
          <GitBranch size={27} />
        </span>
        <h2>Pipeline summary</h2>
        <p>Prepare a new recording from your selected streams.</p>
        <div className="summary-line">
          <span>Input</span>
          <strong>{filename(recording.source_file)}</strong>
        </div>
        <div className="summary-line">
          <span>Selected channels</span>
          <strong>
            {selected.length} / {recording.topics.length}
          </strong>
        </div>
        <div className="summary-line">
          <span>Translations</span>
          <strong>
            {
              Object.entries(workflow.translations).filter(
                ([id, value]) => value && selected.includes(id),
              ).length
            }
          </strong>
        </div>
        <div className="summary-line">
          <span>Synchronization</span>
          <strong>{sync.enabled ? sync.mode : "Off"}</strong>
        </div>
        <div className="pipeline-diagram">
          <span>
            <Layers3 size={18} />
            Source
          </span>
          <ArrowRight size={17} />
          <span>
            <GitBranch size={18} />
            Pipeline
          </span>
          <ArrowRight size={17} />
          <span>
            <CheckCircle2 size={18} />
            Output
          </span>
        </div>
        <p className="field-help">
          The output is added to this project as a separate MCAP recording.
        </p>
        <button
          className="button primary"
          disabled={
            busy ||
            !selected.length ||
            (sync.enabled && !selected.includes(sync.parent_channel))
          }
          onClick={onRun}
        >
          <GitBranch size={17} /> Run pipeline
        </button>
        <button className="button secondary" disabled={busy} onClick={onSave}>
          Save configuration
        </button>
      </aside>
    </div>
  );
}
