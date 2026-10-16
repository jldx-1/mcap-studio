import { useEffect, useRef, useState } from "react";

/** Accept external file drops anywhere without letting the browser open them. */
export function useFileDrop(enabled: boolean, onFiles: (files: File[]) => Promise<void>) {
  const [dragging, setDragging] = useState(false);
  const depth = useRef(0);

  useEffect(() => {
    const hasFiles = (event: DragEvent) => event.dataTransfer?.types.includes("Files");
    const scoped = (event: DragEvent) => event.target instanceof Element && !!event.target.closest("[data-file-drop]");
    const reset = () => {
      depth.current = 0;
      setDragging(false);
    };
    const enter = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      if (scoped(event)) { reset(); return; }
      event.preventDefault();
      depth.current += 1;
      setDragging(true);
    };
    const over = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      if (scoped(event)) { reset(); return; }
      event.preventDefault();
      event.dataTransfer!.dropEffect = enabled ? "copy" : "none";
    };
    const leave = (event: DragEvent) => {
      if (!hasFiles(event) || scoped(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (!depth.current) setDragging(false);
    };
    const drop = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      if (scoped(event)) { reset(); return; }
      event.preventDefault();
      reset();
      void onFiles(Array.from(event.dataTransfer!.files));
    };
    window.addEventListener("dragenter", enter, true);
    window.addEventListener("dragover", over, true);
    window.addEventListener("dragleave", leave, true);
    window.addEventListener("drop", drop, true);
    window.addEventListener("dragend", reset);
    window.addEventListener("blur", reset);
    return () => {
      window.removeEventListener("dragenter", enter, true);
      window.removeEventListener("dragover", over, true);
      window.removeEventListener("dragleave", leave, true);
      window.removeEventListener("drop", drop, true);
      window.removeEventListener("dragend", reset);
      window.removeEventListener("blur", reset);
    };
  }, [enabled, onFiles]);

  return dragging;
}
