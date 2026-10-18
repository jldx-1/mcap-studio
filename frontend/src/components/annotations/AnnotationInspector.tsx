import { Flag, ScanLine, Square, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import type { Tracks } from "../../annotations/state";
import { colors, rebuildTrack, trackInfo } from "../../annotations/state";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";

export function AnnotationInspector({ editor, classes }: {
classes: ReactNode;
editor: Pick<AnnotationEditorController,
    | "state"
    | "frameNumber"
    | "selected"
    | "setSelected"
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
    availableClasses,
    commit,
    updateRow,
    editSelected,
    channel,
    onError,
    onNotice
  } = editor;
  const finishedTracks=Object.entries((state.tracks as Tracks|undefined)?.[channel]||{}).filter(([,info])=>info.end!==null);
  const continueTrack=(id:number)=>{
    let selectedIndex:number|null=null;
    commit(next=>{
      const info=trackInfo(next,channel,id);
      if(!info)return;
      info.end=null;
      rebuildTrack(next,channel,id,info,frames);
      const index=next.annotations[channel]?.[frameNumber]?.boxes.findIndex(box=>box.track_id===id)??-1;
      if(index>=0){selectedIndex=index;setClassId(next.annotations[channel][frameNumber].boxes[index].class_id);}
    });
    setSelected(selectedIndex);setTool("select");
    onNotice(`Track #${id} continues from its last keyframe.`);
  };
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
    </div>
    {classes}
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
                : "Add a class above to start drawing."}
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
              {track?.end!==null&&track?.end!==undefined&&<button className="button secondary small" aria-label="Continue selected track" onClick={()=>continueTrack(selectedBox.track_id!)}>Continue track</button>}
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
        {!!finishedTracks.length&&<div className="finished-tracks"><h4>Finished tracks</h4>{finishedTracks.map(([id,info])=>{
          const first=Object.values(info.keyframes)[0];
          return <article key={id}><div><strong>{first?state.classes[first.class_id]:"Object"} · Track #{id}</strong><small>Finished at frame {info.end!+1}</small></div><button className="button secondary small" aria-label={`Continue track ${id}`} onClick={()=>continueTrack(Number(id))}>Continue track</button></article>;
        })}</div>}
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
            Add a class above or choose an existing label list.
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
  </aside>);
}
