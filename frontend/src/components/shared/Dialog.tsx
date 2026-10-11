import { X } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";

export default function Dialog({
  title,
  children,
  onClose,
  error,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  error?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const before = document.activeElement as HTMLElement;
    ref.current
      ?.querySelector<HTMLElement>("input, button:not(:disabled)")
      ?.focus();
    return () => before?.focus();
  }, []);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
          if (event.key === "Tab") {
            const elements = [
              ...ref.current!.querySelectorAll<HTMLElement>(
                "button:not(:disabled), input, select, textarea",
              ),
            ];
            const first = elements[0],
              last = elements[elements.length - 1];
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <header>
          <h2>{title}</h2>
          <button
            className="icon-button"
            aria-label="Close dialog"
            onClick={onClose}
          >
            <X size={20} />
          </button>
        </header>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        {children}
      </div>
    </div>
  );
}
