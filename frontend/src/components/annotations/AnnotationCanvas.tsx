import { Crosshair, ImageOff, LoaderCircle } from "lucide-react";
import { colors } from "../../annotations/state";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";

export function AnnotationCanvas({ editor }: {  
editor: Pick<AnnotationEditorController,
    | "state"
    | "frameNumber"
    | "selected"
    | "setSelected"
    | "classId"
    | "tool"
    | "record"
    | "recordLoading"
    | "imageFailed"
    | "setImageFailed"
    | "preview"
    | "setPreview"
    | "svg"
    | "drag"
    | "topic"
    | "isImage"
    | "frameRow"
    | "boxTool"
    | "frameUrl"
    | "displayedFrameUrl"
    | "width"
    | "height"
    | "start"
    | "move"
    | "end"
  >
}) {
  const {
    state,
    frameNumber,
    selected,
    setSelected,
    classId,
    tool,
    record,
    recordLoading,
    imageFailed,
    setImageFailed,
    preview,
    setPreview,
    svg,
    drag,
    topic,
    isImage,
    frameRow,
    boxTool,
    frameUrl,
    displayedFrameUrl,
    width,
    height,
    start,
    move,
    end
  } = editor;
  return (<div
    className={`image-stage ${tool === "draw" && boxTool ? "drawing" : ""}`}
  >
    {isImage ? (
      imageFailed ? (
        <div className="image-error">
          <ImageOff size={34} />
          <strong>Frame preview unavailable</strong>
          <span>
            Check that the image format can be decoded by the
            backend.
          </span>
          <button
            className="button secondary small"
            onClick={() => setImageFailed(false)}
          >
            Retry preview
          </button>
        </div>
      ) : (
        <svg
          ref={svg}
          viewBox={`0 0 ${width} ${height}`}
          aria-label={`Annotation canvas for frame ${frameNumber + 1}`}
          role="img"
          onPointerDown={(event) => {
            if (tool === "draw") start(event, "draw");
            else setSelected(null);
          }}
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={() => {
            drag.current = null;
            setPreview(null);
          }}
        >
          <image
            href={displayedFrameUrl || frameUrl}
            width={width}
            height={height}
            onError={() => setImageFailed(true)}
          />
          {boxTool &&
            frameRow.boxes.map((original, index) => {
              const box =
                drag.current?.index === index && preview
                  ? preview
                  : original;
              const color = colors[box.class_id % colors.length];
              const labelSize = Math.max(width / 65, 8);
              const handleSize = Math.max(width / 100, 4);
              return (
                <g
                  key={`${box.track_id}-${index}`}
                  className="annotation-box"
                  onPointerDown={(event) =>
                    start(event, "move", index)
                  }
                >
                  <rect
                    x={box.x}
                    y={box.y}
                    width={box.width}
                    height={box.height}
                    fill={
                      selected === index
                        ? `${color}30`
                        : `${color}10`
                    }
                    stroke={color}
                    strokeWidth={selected === index ? 2.5 : 1.6}
                    vectorEffect="non-scaling-stroke"
                  />
                  <rect
                    x={box.x}
                    y={Math.max(0, box.y - labelSize * 1.6)}
                    width={Math.min(
                      width - box.x,
                      (state.classes[box.class_id]?.length || 5) *
                      labelSize *
                      0.65 +
                      labelSize * 3,
                    )}
                    height={labelSize * 1.6}
                    fill={color}
                    rx={labelSize / 5}
                  />
                  <text
                    x={box.x + labelSize / 3}
                    y={Math.max(
                      labelSize * 1.2,
                      box.y - labelSize * 0.4,
                    )}
                    fill="white"
                    fontSize={labelSize}
                    fontWeight="600"
                  >
                    {state.classes[box.class_id]} #{box.track_id}
                  </text>
                  {selected === index &&
                    [
                      ["nw", box.x, box.y],
                      ["ne", box.x + box.width, box.y],
                      ["sw", box.x, box.y + box.height],
                      ["se", box.x + box.width, box.y + box.height],
                    ].map(([corner, x, y]) => (
                      <rect
                        key={String(corner)}
                        x={Number(x) - handleSize / 2}
                        y={Number(y) - handleSize / 2}
                        width={handleSize}
                        height={handleSize}
                        fill="white"
                        stroke={color}
                        strokeWidth="1.5"
                        vectorEffect="non-scaling-stroke"
                        className={`resize-handle ${corner}`}
                        onPointerDown={(event) =>
                          start(
                            event,
                            "resize",
                            index,
                            String(corner),
                          )
                        }
                      />
                    ))}
                </g>
              );
            })}
          {drag.current?.kind === "draw" && preview && (
            <rect
              x={preview.x}
              y={preview.y}
              width={preview.width}
              height={preview.height}
              stroke={colors[classId % colors.length]}
              fill={`${colors[classId % colors.length]}25`}
              strokeWidth="2"
              vectorEffect="non-scaling-stroke"
            />
          )}
        </svg>
      )
    ) : (
      <div className="record-preview">
        {recordLoading ? (
          <LoaderCircle className="spin" size={24} />
        ) : (
          <>
            <div className="record-preview-heading">
              <span>MESSAGE PAYLOAD</span>
              <code>{topic?.encoding}</code>
            </div>
            <pre>{JSON.stringify(record, null, 2)}</pre>
          </>
        )}
      </div>
    )}
    {isImage && !imageFailed && (
      <div className="image-badge">
        <Crosshair size={13} />
        {width} × {height}
      </div>
    )}
  </div>);
}
