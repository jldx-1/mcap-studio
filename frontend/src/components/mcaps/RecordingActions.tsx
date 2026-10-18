import { MoreHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
export function RecordingActions({name,actions}:{name:string;actions:{label:string;run:()=>void;disabled?:boolean;danger?:boolean}[]}) {
  const [position,setPosition]=useState<{top:number;left:number}|null>(null),trigger=useRef<HTMLButtonElement>(null),menu=useRef<HTMLDivElement>(null);
  const close=(focus=false)=>{setPosition(null);if(focus)trigger.current?.focus();};
  useEffect(()=>{
    if(!position)return;
    const pointer=(event:PointerEvent)=>{if(!menu.current?.contains(event.target as Node)&&!trigger.current?.contains(event.target as Node))close();};
    const key=(event:KeyboardEvent)=>{if(event.key==="Escape")close(true);};
    const scroll=(event:Event)=>{if(!menu.current?.contains(event.target as Node))close();};
    document.addEventListener("pointerdown",pointer);document.addEventListener("keydown",key);window.addEventListener("scroll",scroll,true);window.addEventListener("resize",scroll);
    return()=>{document.removeEventListener("pointerdown",pointer);document.removeEventListener("keydown",key);window.removeEventListener("scroll",scroll,true);window.removeEventListener("resize",scroll);};
  },[position]);
  return <><button ref={trigger} type="button" className="icon-button" aria-label={`Actions for ${name}`} aria-haspopup="menu" aria-expanded={!!position} onClick={()=>{
    if(position){close();return;}const rect=trigger.current!.getBoundingClientRect(),height=actions.length*38+12;
    setPosition({left:Math.max(8,Math.min(rect.right-170,window.innerWidth-178)),top:rect.bottom+height<window.innerHeight?rect.bottom+4:Math.max(8,rect.top-height-4)});
    requestAnimationFrame(()=>menu.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus());
  }}><MoreHorizontal size={18}/></button>{position&&createPortal(<div ref={menu} role="menu" aria-label={`Recording actions for ${name}`} className="recording-floating-menu" style={position} onKeyDown={event=>{if(["ArrowDown","ArrowUp","Home","End"].includes(event.key)){event.preventDefault();const buttons=[...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")],index=buttons.indexOf(document.activeElement as HTMLButtonElement),next=event.key==="Home"?0:event.key==="End"?buttons.length-1:(index+(event.key==="ArrowDown"?1:buttons.length-1))%buttons.length;buttons[next]?.focus();}}}>{actions.map(action=><button role="menuitem" key={action.label} disabled={action.disabled} className={action.danger?"danger-text":""} onClick={()=>{close();action.run();}}>{action.label}</button>)}</div>,document.body)}</>;
}
