import { useMemo, useState } from "react";
import { Plus } from "lucide-react";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";
import type { Project } from "../../types/models";
import { post } from "../../services/http";
import { can } from "../../utils/permissions";
import { useAuth } from "../../state/AuthContext";

export function EditorClasses({editor,project,onProject}:{editor:AnnotationEditorController;project:Project;onProject:(value:Project)=>Promise<void>}) {
  const {user}=useAuth();
  const [name,setName]=useState(""),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const {state,channel,assignedLabelList,availableClasses}=editor;
  const used=useMemo(()=>new Set(Object.values(state.annotations[channel]||{}).flatMap(row=>[...row.boxes.map(box=>box.class_id),...(row.labels||[])]).map(id=>state.class_uuids?.[id])),[state.annotations,channel,state.class_uuids]);
  return <div className="editor-classes">
    <h3>Classes</h3>
    <label>Label list<select aria-label="Editor label list" disabled={busy} value={assignedLabelList?.uuid||""} onChange={event=>editor.commit(next=>{
      const list=project.label_lists.find(item=>item.uuid===event.target.value);
      next.topic_label_lists||={};next.topic_labels||={};
      if(list){next.topic_label_lists[channel]=list.uuid;next.topic_labels[channel]=[...list.label_ids];}
      else {delete next.topic_label_lists[channel];delete next.topic_labels[channel];}
    })}><option value="">All project classes</option>{project.label_lists.map(list=><option key={list.uuid} value={list.uuid} disabled={[...used].some(id=>!id||!list.label_ids.includes(id))}>{list.name}</option>)}</select></label>
    {editor.boxTool&&<label>Active class<select aria-label="Active annotation class" disabled={!availableClasses.length||busy} value={editor.classId} onChange={event=>{const id=Number(event.target.value);editor.setClassId(id);if(editor.selectedBox)editor.editSelected({class_id:id});}}>{availableClasses.length?availableClasses.map(item=><option key={item.id} value={item.id}>{item.name}</option>):<option value="0">Add a class to start drawing</option>}</select></label>}
    {can(project,user,"labels.manage")&&<form className="editor-add-class" onSubmit={event=>{event.preventDefault();if(!name.trim()||busy)return;setBusy(true);setError("");void (async()=>{
      const updated=await post<Project>(`/projects/${project.uuid}/${assignedLabelList?`label-lists/${assignedLabelList.uuid}/labels`:"labels"}`,{name:name.trim()});
      const label=updated.labels.find(item=>item.name===name.trim());
      if(!label)throw new Error("Class was not returned by the server. Refresh classes and try again.");
      await onProject(updated);
      let active=0;
      editor.commit(next=>{
        next.class_uuids||=next.classes.map(()=>null);
        active=next.class_uuids.indexOf(label.uuid);
        if(active<0){active=next.classes.length;next.classes.push(label.name);next.class_uuids.push(label.uuid);}
        if(assignedLabelList){next.topic_labels||={};next.topic_labels[channel]=updated.label_lists.find(item=>item.uuid===assignedLabelList.uuid)!.label_ids;}
        else if(next.topic_labels?.[channel])next.topic_labels[channel]=[...new Set([...next.topic_labels[channel],label.uuid])];
      });
      editor.setClassId(active);editor.setSelected(null);setName("");editor.onNotice(`Class ${label.name} added.`);
    })().catch(error=>setError(error instanceof Error?error.message:String(error))).finally(()=>setBusy(false));}}>
      <input aria-label="New annotation class" placeholder="New class name" value={name} maxLength={120} disabled={busy} onChange={event=>setName(event.target.value)}/><button className="button secondary small" aria-label="Add class" disabled={busy||!name.trim()}><Plus size={14}/>Add</button>
    </form>}
    {error&&<p className="inline-error" role="alert">{error}</p>}
    {!availableClasses.length&&<p className="field-help">Add a class here or choose an existing label list.</p>}
  </div>;
}
