import { Check, Copy, X } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export function TopicErrors({ topic, channel, errors }: { topic: string; channel: string; errors: string[] }) {
  const text = `Topic: ${topic}\nChannel: ${channel}\n\n${errors.join("\n")}`;
  const id = useId();
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const [copied, setCopied] = useState("");
  const [copying, setCopying] = useState(false);
  const [notice, setNotice] = useState("");
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cancelClose = () => clearTimeout(timer.current);
  const show = () => { cancelClose(); setOpen(true); };
  const contains = (target: EventTarget | null) => target instanceof Node && (trigger.current?.contains(target) || popup.current?.contains(target));
  const hideSoon = () => {
    cancelClose();
    timer.current = setTimeout(() => { if (!contains(document.activeElement)) setOpen(false); }, 250);
  };
  const close = () => { cancelClose(); trigger.current?.focus(); setOpen(false); };
  useEffect(() => () => clearTimeout(timer.current), []);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const anchor = trigger.current?.getBoundingClientRect();
      const panel = popup.current?.getBoundingClientRect();
      if (!anchor || !panel) return;
      const left = Math.max(8, Math.min(anchor.left, window.innerWidth - panel.width - 8));
      const below = anchor.bottom + 8;
      const top = below + panel.height <= window.innerHeight - 8 ? below : Math.max(8, anchor.top - panel.height - 8);
      setPosition({ top, left });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [open, text, notice, copied]);
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => { if (!contains(event.target)) { cancelClose(); setOpen(false); } };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", dismiss); document.removeEventListener("keydown", escape); };
  }, [open]);
  const copy = async () => {
    setCopying(true); setCopied(""); setNotice("");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(text);
    } catch {
      // HTTP LAN connections may lack clipboard access. Keep the popup open
      // with the complete text selected for browser or keyboard copying.
      field.current?.focus(); field.current?.select();
      let success = false;
      try { success = document.execCommand("copy"); } catch { /* Leave the text selected. */ }
      if (success) setCopied(text);
      else setNotice("Press Ctrl+C or ⌘C to copy the selected error text.");
    } finally { setCopying(false); }
  };
  return <div className="topic-errors">
    <button type="button" ref={trigger} className="pill amber topic-error-trigger" aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined} onMouseEnter={show} onMouseLeave={hideSoon} onFocus={show} onBlur={event => { if (!contains(event.relatedTarget)) hideSoon(); }} onClick={show}>Needs attention</button>
    {open && createPortal(<div ref={popup} id={id} className="topic-error-popup" role="dialog" aria-label={`Error details for ${topic}`} style={position} onMouseEnter={cancelClose} onMouseLeave={event => { if (!event.buttons) hideSoon(); }} onFocus={cancelClose} onBlur={event => { if (!contains(event.relatedTarget)) hideSoon(); }}>
      <div className="topic-error-heading"><strong>Error details</strong><button type="button" className="icon-button" aria-label="Close error details" onClick={close}><X size={16}/></button></div>
      <textarea ref={field} aria-label={`Error message for ${topic}`} readOnly rows={6} value={text}/>
      <div className="topic-error-footer"><button type="button" className="button secondary small" aria-label={`Copy error for ${topic}`} disabled={copying} onClick={() => void copy()}>{copied === text ? <Check size={14}/> : <Copy size={14}/>} {copied === text ? "Copied" : "Copy error"}</button><span className="field-help" role="status">{notice || (copied === text ? "Error copied to clipboard." : "Select text to copy any part of the message.")}</span></div>
    </div>, document.body)}
  </div>;
}
