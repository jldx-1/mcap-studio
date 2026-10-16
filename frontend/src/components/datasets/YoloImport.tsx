import { useState } from "react";
import { Upload, X } from "lucide-react";
import { api } from "../../services/http";
import type { Dataset, Project } from "../../types/models";
import type { Perform } from "../../types/workspace";

const filePath = (file: File) => file.webkitRelativePath || file.name;

export function YoloImport({project,busy,perform,onProject,onImported}: {
  project: Project; busy: boolean; perform: Perform;
  onProject: (project: Project) => Promise<void>; onImported: (dataset: Dataset) => void;
}) {
  const [name,setName] = useState("");
  const [files,setFiles] = useState<File[]>([]);
  const [classes,setClasses] = useState("");
  const addFiles = (incoming: File[]) => {
    if (busy) return;
    setFiles(current => [...new Map([...current,...incoming].map(file => [filePath(file),file])).values()]);
    if (!name && incoming.length) setName((incoming[0].webkitRelativePath.split('/')[0] || incoming[0].name.replace(/\.[^.]+$/,"")) + " dataset");
  };
  const images = files.filter(file => /\.(png|jpe?g)$/i.test(file.name)).length;
  const labels = files.filter(file => /\.txt$/i.test(file.name) && file.name !== "classes.txt").length;
  const zip = files.some(file => /\.zip$/i.test(file.name));
  return <section className="panel" data-file-drop="yolo" onDragOver={event=>{event.preventDefault();event.dataTransfer.dropEffect=busy?"none":"copy";}} onDrop={event=>{event.preventDefault();event.stopPropagation();addFiles(Array.from(event.dataTransfer.files));}}>
    <div className="panel-heading"><div><h2>Import a YOLO dataset</h2><p>Load detection labels with their corresponding PNG/JPEG frames.</p></div><Upload size={22}/></div>
    <form className="collection-body account-form" onSubmit={event=>{event.preventDefault();void perform("Importing YOLO dataset",async()=>{
      const form = new FormData();
      form.append("name",name.trim());
      form.append("class_names",JSON.stringify(classes.split(/\r?\n/).map(line=>line.trim()).filter(Boolean)));
      files.forEach(file=>form.append("file",file,filePath(file)));
      const result = await api<{dataset:Dataset;project:Project}>(`/projects/${project.uuid}/datasets/import-yolo`,{method:"POST",body:form});
      await onProject(result.project);onImported(result.dataset);
    });}}>
      <label>Dataset name<input required value={name} maxLength={120} disabled={busy} onChange={event=>setName(event.target.value)} placeholder="e.g. Warehouse objects"/></label>
      <div className="yolo-file-picker"><p>Drop a ZIP or select a dataset folder containing images, labels, and an optional data.yaml or classes.txt. You can also select label files and images together.</p><div className="button-row">
        <label className="button secondary">Choose ZIP or files<input type="file" className="visually-hidden" aria-label="YOLO dataset files" multiple accept=".zip,.txt,.names,.yaml,.yml,.png,.jpg,.jpeg" disabled={busy} onChange={event=>{addFiles(Array.from(event.target.files||[]));event.target.value="";}}/></label>
        <label className="button secondary">Choose dataset folder<input type="file" className="visually-hidden" aria-label="YOLO dataset folder" multiple ref={node=>{node?.setAttribute("webkitdirectory","");}} disabled={busy} onChange={event=>{addFiles(Array.from(event.target.files||[]));event.target.value="";}}/></label>
      </div></div>
      {!!files.length&&<div className="button-row"><span>{zip?`${files.length} file${files.length===1?"":"s"} selected`:`${images} images · ${labels} label files · ${files.length} files total`}</span><button type="button" className="text-button" disabled={busy} onClick={()=>setFiles([])}><X size={14}/>Clear files</button></div>}
      <details><summary>Class names · optional</summary><label>One class name per line, in class-ID order<textarea rows={4} value={classes} disabled={busy} onChange={event=>setClasses(event.target.value)} placeholder={"person\nvehicle"}/></label><p className="field-help">Names are read from data.yaml, classes.txt, or obj.names when available. Otherwise class IDs receive names such as Class 0. Supplied names override the class file.</p></details>
      <p className="field-help">Labels use class x_center y_center width height with coordinates from 0 to 1. Images without labels are kept as negative examples. Imported data stays in its own dataset.</p>
      <button className="button primary" disabled={busy||!name.trim()||!files.length}>Import YOLO dataset</button>
    </form>
  </section>;
}
