import { CheckCircle2, Flag, ScanLine, Square, Trash2 } from "lucide-react";
import type { Tracks } from "../../annotations/state";
import { colors, rebuildTrack, trackInfo } from "../../annotations/state";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";

export function AnnotationInspector({ editor }: {  
editor: Pick<AnnotationEditorController,
    | "state"
    | "frameNumber"
    | "selected"
    | "setSelected"
    | "classId"
    | "setClassId"
    | "setTool"
    | "setDirty"
    | "fields"
    | "setFields"
    | "fieldsRef"
    | "dirtyRef"
    | "frames"
    | "frameRow"
    | "technique"
    | "boxTool"
    | "selectedBox"
    | "track"
    | "assignedLabelList"
    | "availableClasses"
    | "commit"
    | "updateRow"
    | "editSelected"
    | "channel"
    | "onError"
    | "onNotice"
  >
}) {
  const {
    state,
    frameNumber,
    selected,
    setSelected,
    classId,
    setClassId,
    setTool,
    setDirty,
    fields,
    setFields,
    fieldsRef,
    dirtyRef,
    frames,
    frameRow,
    technique,
    boxTool,
    selectedBox,
    track,
    assignedLabelList,
    availableClasses,
    commit,
    updateRow,
    editSelected,
    channel,
    onError,
    onNotice
  } = editor;
  return (<aside className="annotation-sidebar">
    <div className="annotation-settings">
      <label>
        Annotation type
        <select
          aria-label="Annotation type"
          value={technique}
          disabled={Object.values(
            state.annotations[channel] || {},
          ).some((row) => row.boxes?.length || row.labels?.length)}
          onChange={(event) =>
            commit((next) => {
              next.techniques[channel] = event.target.value;
            })
          }
        >
          <option value="yolo">Bounding boxes · YOLO</option>
          <option value="classification">Video classification</option>
        </select>
      </label>
      <div className="assigned-label-list">
        <span>Label list</span>
        <strong>{assignedLabelList?.name || "No list assigned"}</strong>
        {!assignedLabelList && <p className="field-help">Choose a label list in the data labeling table before adding labels.</p>}
      </div>
      {boxTool && (
        <label>
          Active class
          <select
            aria-label="Active annotation class"
            value={classId}
            onChange={(event) => {
              const id = Number(event.target.value);
              setClassId(id);
              if (selectedBox) editSelected({ class_id: id });
            }}
          >
            {availableClasses.length ? (
              availableClasses.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))
            ) : (
              <option value="0">Choose a list in the data labeling table</option>
            )}
          </select>
        </label>
      )}
    </div>
    <div className="annotation-list-heading">
      <h3>{boxTool ? "Annotations" : "Frame classes"}</h3>
      <span className="pill">
        {boxTool
          ? frameRow.boxes.length
          : frameRow.labels?.length || 0}
      </span>
    </div>
    {boxTool ? (
      <div className="annotation-list">
        {frameRow.boxes.length ? (
          frameRow.boxes.map((box, index) => (
            <button
              key={`${box.track_id}-${index}`}
              className={`box-row ${selected === index ? "selected" : ""}`}
              onClick={() => {
                setSelected(index);
                setClassId(box.class_id);
                setTool("select");
              }}
            >
              <span
                className="box-color"
                style={{
                  background: colors[box.class_id % colors.length],
                }}
              />
              <div>
                <strong>
                  {state.classes[box.class_id] ||
                    `Class ${box.class_id}`}
                </strong>
                <span>
                  Track #{box.track_id} ·{" "}
                  {box.origin === "manual" || !box.origin
                    ? "Keyframe"
                    : box.origin}
                </span>
              </div>
              <ScanLine size={15} />
            </button>
          ))
        ) : (
          <div className="annotations-empty">
            <Square size={24} />
            <p>
              {availableClasses.length
                ? "Draw your first box on the frame."
                : "Choose a list in the data labeling table, then add classes to that list to start drawing."}
            </p>
          </div>
        )}
        {selectedBox && (
          <div className="box-properties">
            <h4>Selected box</h4>
            <div className="coordinate-grid">
              {(["x", "y", "width", "height"] as const).map((key) => (
                <label key={key}>
                  {key === "width"
                    ? "Width"
                    : key === "height"
                      ? "Height"
                      : key.toUpperCase()}
                  <input
                    type="number"
                    step="0.1"
                    min={key === "width" || key === "height" ? 1 : 0}
                    aria-label={`Box ${key}`}
                    value={Number(selectedBox[key].toFixed(2))}
                    onChange={(event) =>
                      editSelected({
                        [key]: Number(event.target.value),
                      })
                    }
                  />
                </label>
              ))}
            </div>
            <div className="track-detail">
              <span>Track #{selectedBox.track_id}</span>
              <span>
                Entry {Number(track?.start || 0) + 1} · Exit{" "}
                {track?.end === null || track?.end === undefined
                  ? "End"
                  : track.end + 1}
              </span>
            </div>
            <div className="track-actions">
              <button
                className="button secondary small"
                onClick={() =>
                  commit((next) => {
                    const info = trackInfo(
                      next,
                      channel,
                      selectedBox.track_id!,
                    );
                    if (!info) return;
                    info.end = frameNumber;
                    info.keyframes = Object.fromEntries(
                      Object.entries(info.keyframes).filter(
                        ([key]) => Number(key) <= frameNumber,
                      ),
                    );
                    info.keyframes[frameNumber] = {
                      ...selectedBox,
                      origin: "manual",
                    };
                    rebuildTrack(
                      next,
                      channel,
                      selectedBox.track_id!,
                      info,
                      frames,
                    );
                  })
                }
              >
                <Flag size={14} /> End here
              </button>
              <button
                className="button secondary small"
                onClick={() =>
                  commit((next) => {
                    const info = trackInfo(
                      next,
                      channel,
                      selectedBox.track_id!,
                    );
                    if (!info) return;
                    info.end = null;
                    rebuildTrack(
                      next,
                      channel,
                      selectedBox.track_id!,
                      info,
                      frames,
                    );
                  })
                }
              >
                Continue
              </button>
            </div>
            <p className="field-help">
              Edits create keyframes. Boxes interpolate by timestamp
              between them and continue to the track exit.
            </p>
            <button
              className="text-button danger"
              onClick={() => {
                commit((next) => {
                  for (const row of Object.values(
                    next.annotations[channel] || {},
                  ))
                    row.boxes = row.boxes.filter(
                      (box) => box.track_id !== selectedBox.track_id,
                    );
                  delete (next.tracks as Tracks | undefined)?.[
                    channel
                  ]?.[String(selectedBox.track_id)];
                });
                setSelected(null);
              }}
            >
              <Trash2 size={14} /> Delete entire track
            </button>
          </div>
        )}
      </div>
    ) : (
      <div className="classifications">
        {availableClasses.length ? (
          availableClasses.map((item) => (
            <label className="check-label" key={item.id}>
              <input
                type="checkbox"
                checked={frameRow.labels?.includes(item.id) || false}
                onChange={(event) =>
                  updateRow((row) => {
                    row.labels = event.target.checked
                      ? [...(row.labels || []), item.id]
                      : (row.labels || []).filter(
                        (value) => value !== item.id,
                      );
                  })
                }
              />
              <span
                className="box-color"
                style={{
                  background: colors[item.id % colors.length],
                }}
              />
              {item.name}
            </label>
          ))
        ) : (
          <p className="muted">
            Assign a list in the data labeling table and create labels inside that list.
          </p>
        )}
        {technique === "jsonl" && (
          <>
            <label>
              Annotation fields (JSON)
              <textarea
                aria-label="Record annotation fields"
                rows={7}
                value={fields}
                onChange={(event) => {
                  setFields(event.target.value);
                  fieldsRef.current = event.target.value;
                  setDirty(true);
                  dirtyRef.current = true;
                }}
              />
            </label>
            <button
              className="button secondary small"
              onClick={() => {
                try {
                  const value = JSON.parse(fields);
                  updateRow((row) => {
                    row.fields = value;
                  });
                  onNotice(
                    "Record fields applied. Save annotations to persist them.",
                  );
                } catch {
                  onError("Annotation fields must be valid JSON.");
                }
              }}
            >
              Apply fields
            </button>
          </>
        )}
      </div>
    )}
    <div className="review-panel">
      <p className="field-help">{frameRow.skipped ? "Skipped — excluded from training" : frameRow.explicit_empty ? "Confirmed empty — training negative" : frameRow.boxes.length || frameRow.labels?.length ? "Labeled frame" : "Unlabeled frame"}. Completion is separate from reviewer approval.</p>
      <label className="check-label"><input type="checkbox" checked={frameRow.skipped === true} onChange={e => updateRow(row => { row.skipped = e.target.checked; if (e.target.checked) { row.reviewed = false; delete row.explicit_empty; } })} />Skip this frame</label>
      <label
        className={`review-check ${frameRow.reviewed ? "is-reviewed" : ""}`}
      >
        <input
          type="checkbox"
          checked={frameRow.reviewed}
          onChange={(event) =>
            updateRow((row) => {
              row.reviewed = event.target.checked;
              if (event.target.checked) row.skipped = false;
              if (
                event.target.checked &&
                !row.boxes.length &&
                !(row.labels || []).length
              )
                row.explicit_empty = true;
              else delete row.explicit_empty;
            })
          }
        />
        <CheckCircle2 size={20} />
        <div>
          <strong>Frame complete</strong>
          <span>
            {frameRow.reviewed
              ? "Included in the next task submission"
              : "Optional progress marker; empty completed frames become training negatives"}
          </span>
        </div>
      </label>

    </div>
  </aside>);
}
