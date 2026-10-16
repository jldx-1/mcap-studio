import {
  ArrowLeft,
  Check,
  CircleHelp,
  Hand,
  ImageOff,
  LoaderCircle,
  Save,
  Square,
  Trash2,
  Undo2
} from "lucide-react";
import { useAnnotationEditor } from "../../hooks/useAnnotationEditor";
import type { AnnotationEditorProps } from "../../types/editor";
import { useReviewTasks } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import { can } from "../../utils/permissions";
import { SubmissionPanel } from "../reviews/SubmissionPanel";
import Dialog from "../shared/Dialog";
import { AnnotationCanvas } from "./AnnotationCanvas";
import { AnnotationInspector } from "./AnnotationInspector";
import { AnnotationTimeline } from "./AnnotationTimeline";

export default function AnnotationEditor(props: AnnotationEditorProps) {
  const { user } = useAuth();
  const taskAccess = can(props.project,user,"annotation.view") || can(props.project,user,"review.view");
  const reviews = useReviewTasks(props.project.uuid,props.onError,taskAccess);
  const editor = useAnnotationEditor({ ...props,readOnly: props.readOnly || reviews.loading });
  const {
    selected,
    tool,
    setTool,
    dirty,
    saving,
    leavePrompt,
    saveError,
    history,
    help,
    setHelp,
    frames,
    topic,
    technique,
    boxTool,
    undo,
    save,
    resolveLeave,
    deleteBox,
    onBack,
    onLabels
  } = editor;
  return (
    <section className="panel editor-panel">
      <div className="editor-header">
        <div className="topic-picker">
          <button
            className="button secondary small"
            disabled={saving}
            onClick={onBack}
          >
            <ArrowLeft size={16} /> Back To Table
          </button>
          <strong>{topic?.topic}</strong>
          {props.onNext && <button className="button secondary small" disabled={saving} onClick={() => void (async () => { if (editor.readOnly || await save()) await props.onNext?.(); })()}>Save and next topic / recording</button>}
          {can(props.project,user,"labels.manage") && <button
            className="button secondary small"
            disabled={saving}
            onClick={onLabels}
          >
            Label lists…
          </button>}
        </div>
        {!editor.readOnly && <div className="editor-save">
          <span className={`save-state ${dirty ? "unsaved" : ""}`}>
            {dirty ? <span /> : <Check size={14} />}
            {saveError ? "Save failed — retry to continue" : saving ? "Saving…" : dirty ? "Autosave pending" : "All changes saved"}
          </span>
          <button
            className="button primary small"
            disabled={saving || !frames.length}
            onClick={() => void save()}
          >
            {saving ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Save size={16} />
            )}{" "}
            Save annotations
          </button>
        </div>}
      </div>
      {editor.readOnly && <p className="field-help collection-body">{"Viewing saved annotations."}</p>}
      {topic?.errors?.length ? (
        <div className="inline-error">{topic.errors.join(" · ")}</div>
      ) : null}
      {!frames.length ? (
        <div className="empty">
          <span className="empty-icon">
            <ImageOff size={28} />
          </span>
          <h3>No frames available for this topic</h3>
          <p>Choose a supported image or record channel to start labeling.</p>
        </div>
      ) : (
        <>
          <div className="editor-workspace">
            <div className="canvas-column">
              <div className="canvas-toolbar">
                <fieldset className="tool-group editor-tools" disabled={editor.readOnly}>
                  {boxTool && (
                    <>
                      <button
                        className={`icon-button ${tool === "select" ? "active-tool" : ""}`}
                        aria-label="Select and move boxes (V)"
                        title="Select & move (V)"
                        onClick={() => setTool("select")}
                      >
                        <Hand size={18} />
                      </button>
                      <button
                        className={`icon-button ${tool === "draw" ? "active-tool" : ""}`}
                        aria-label="Draw bounding box (B)"
                        title="Draw box (B)"
                        onClick={() => setTool("draw")}
                      >
                        <Square size={18} />
                      </button>
                      <span className="tool-divider" />
                    </>
                  )}
                  <button
                    className="icon-button"
                    aria-label="Undo (Ctrl+Z)"
                    title="Undo (Ctrl+Z)"
                    disabled={!history.length}
                    onClick={undo}
                  >
                    <Undo2 size={18} />
                  </button>
                  {boxTool && (
                    <button
                      className="icon-button danger"
                      aria-label="Delete selected box"
                      title="Delete selected box"
                      disabled={selected === null}
                      onClick={deleteBox}
                    >
                      <Trash2 size={18} />
                    </button>
                  )}
                </fieldset>
                <span className="canvas-hint">
                  {editor.readOnly ? "Read-only annotations" : boxTool
                    ? tool === "draw"
                      ? "Drag to draw a box"
                      : "Select a box to move or resize"
                    : technique === "classification"
                      ? "Assign classes to this frame"
                      : "Annotate the record below"}
                </span>
                <button
                  className="icon-button"
                  aria-label="Keyboard shortcuts"
                  title="Keyboard shortcuts"
                  onClick={() => setHelp(!help)}
                >
                  <CircleHelp size={18} />
                </button>
              </div>
              {help && (
                <div className="shortcut-bar">
                  <span>
                    <kbd>←</kbd>
                    <kbd>→</kbd> Seek
                  </span>
                  <span>
                    <kbd>Space</kbd> Play
                  </span>
                  <span>
                    <kbd>B</kbd> Draw
                  </span>
                  <span>
                    <kbd>V</kbd> Select
                  </span>
                  <span>
                    <kbd>R</kbd> Mark complete
                  </span>
                  <span>
                    <kbd>Ctrl S</kbd> Save
                  </span>
                </div>
              )}
              <AnnotationCanvas editor={editor} />
              <AnnotationTimeline editor={editor} />
            </div>
            <fieldset className="editor-inspector" disabled={editor.readOnly}><AnnotationInspector editor={editor} /></fieldset>
          </div>
        </>
      )}
      {taskAccess && <SubmissionPanel project={props.project} recording={{...props.recording,annotations:editor.state}} channel={props.channel} controller={reviews} save={save} onError={props.onError} onJump={(channel,frame) => { if(channel === props.channel) editor.navigateFrame(frame); }} />}
      {leavePrompt && (
        <Dialog
          title="Save annotations before leaving?"
          onClose={() => resolveLeave(false)}
        >
          <p className="modal-description">
            Save your annotations before leaving this topic. Discard removes
            unsaved edits; Cancel keeps this topic open.
          </p>
          {saveError && (
            <p className="inline-error" role="alert">
              {saveError}
            </p>
          )}
          <div className="modal-footer">
            <button
              className="button secondary"
              disabled={saving}
              onClick={() => resolveLeave(false)}
            >
              Cancel
            </button>
            <button
              className="button secondary"
              disabled={saving}
              onClick={() => resolveLeave(true)}
            >
              Discard changes
            </button>
            <button
              className="button primary"
              disabled={saving}
              onClick={() =>
                void (async () => {
                  if (await save()) resolveLeave(true);
                })()
              }
            >
              {saving ? "Saving…" : "Save annotations"}
            </button>
          </div>
        </Dialog>
      )}
    </section>
  );

}
