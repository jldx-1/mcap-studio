import { useState } from "react";
import type { useApprovedData } from "../../hooks/useApprovedData";
import { post } from "../../services/http";
import type { ApprovedSource,DatasetVersion } from "../../types/datasetVersions";
import type { Dataset,Project } from "../../types/models";
import type { Perform } from "../../types/workspace";
export type ReturnTypeOfApproved=ReturnType<typeof useApprovedData>;

export function DatasetCreation({project,target,sources,data,busy,perform,onCreated,onProject}:{project:Project;target:Dataset|null;sources:ApprovedSource[];data:ReturnTypeOfApproved;busy:boolean;perform:Perform;onCreated:(dataset:Dataset,version:DatasetVersion)=>void;onProject:(project:Project)=>Promise<void>}){
  const [name,setName]=useState("");const [modalities,setModalities]=useState<string[]>(data.modalities.length?data.modalities:[sources[0]?.modalities.find((m)=>m.annotated_samples>0)?.id||sources[0]?.modalities[0]?.id||"rgb"]);
  const [labels,setLabels]=useState<string[]>(data.labels);const [negatives,setNegatives]=useState(true);const [primary,setPrimary]=useState("");
  const [mode,setMode]=useState("reviewed"),[tolerance,setTolerance]=useState(100),[window,setWindow]=useState(0);
  const options=[...new Map(sources.flatMap((s)=>s.modalities).map((m)=>[m.id,{id:m.id,name:m.name}])).values()];
  const labelOptions=[...new Map(sources.flatMap((s)=>s.labels).map((l)=>[l.id,l])).values()];
  const compatible=sources.filter((s)=>modalities.every((m)=>s.modalities.some((input)=>input.id===m)));
  const estimate=compatible.reduce((sum,s)=>sum+s.modalities.filter((m)=>m.annotated_samples>0&&(!primary||m.id===primary)).reduce((count,m)=>count+(labels.length?m.labels.filter((l)=>labels.includes(l.id)).reduce((n,l)=>n+l.samples,0):m.annotated_samples),0),0);
  const toggle=(items:string[],id:string,checked:boolean)=>checked?[...items,id]:items.filter((value)=>value!==id);
  return <section className="panel dataset-creation"><div className="panel-heading"><div><h2>{target?`Create version ${(target.latest_version||0)+1} of ${target.name}`:"Create dataset from approved data"}</h2><p>Every version keeps its source revisions and input recipe. New approvals never change an existing version.</p></div></div><form className="account-form" onSubmit={(e)=>{e.preventDefault();void perform("Creating immutable dataset version",async()=>{
    const result=await post<{dataset:Dataset;version:DatasetVersion;project:Project}>(target?`/projects/${project.uuid}/datasets/${target.uuid}/versions`:`/projects/${project.uuid}/dataset-versions`,{name:name.trim(),sources:compatible.map((s)=>({project_uuid:s.project_uuid,task_id:s.task_id,revision:s.revision,snapshot_sha256:s.snapshot_sha256})),modalities,label_ids:labels,include_negatives:negatives,primary_modality:primary||null,sensor_window_ns:window?Math.round(window*1e6):null,synchronization:mode==="reviewed"?null:{mode,tolerance_ns:Math.round(tolerance*1e6)}});
    await onProject(result.project);onCreated(result.dataset,result.version);
  });}}>
    {!target&&<label>Dataset name<input required value={name} onChange={(e)=>setName(e.target.value)} maxLength={120} placeholder="gripper_detection" disabled={busy}/></label>}
    <div className="approved-filters"><fieldset disabled={busy}><legend>Input modalities</legend>{options.map((m)=><label key={m.id} className="check-label"><input type="checkbox" checked={modalities.includes(m.id)} onChange={(e)=>setModalities(toggle(modalities,m.id,e.target.checked))}/>{m.name}</label>)}</fieldset><fieldset disabled={busy}><legend>Target labels</legend><p className="field-help">Leave unselected to include all approved labels.</p>{labelOptions.map((l)=><label key={l.id} className="check-label"><input type="checkbox" checked={labels.includes(l.id)} onChange={(e)=>setLabels(toggle(labels,l.id,e.target.checked))}/>{l.name}</label>)}</fieldset></div>
    <div className="approved-filter-controls"><label>Target stream<select value={primary} onChange={(e)=>setPrimary(e.target.value)} disabled={busy}><option value="">All directly annotated streams</option>{options.map((m)=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label><label>Timestamp synchronization<select value={mode} onChange={(e)=>setMode(e.target.value)} disabled={busy}><option value="reviewed">Reviewed project configuration</option><option value="nearest">Nearest timestamp</option><option value="linear">Linear interpolation</option><option value="rate">Rate extrapolation</option></select></label>{mode!=="reviewed"&&<label>Maximum timestamp gap (ms)<input type="number" min={0} value={tolerance} onChange={(e)=>setTolerance(Number(e.target.value))} disabled={busy}/></label>}<label>Sensor window (ms)<input type="number" min={0} max={10000} value={window} onChange={(e)=>setWindow(Number(e.target.value))} disabled={busy}/></label></div>
    <label className="check-label"><input type="checkbox" checked={negatives} onChange={(e)=>setNegatives(e.target.checked)} disabled={busy}/>Include explicitly annotated empty frames as negatives</label>
    <div className="dataset-estimate"><strong>{estimate.toLocaleString()} estimated target samples</strong><span>{compatible.length} compatible approved revisions · {modalities.length} modalities</span></div>
    {compatible.length<sources.length&&<p className="field-help">{sources.length-compatible.length} selected revisions lack one of the requested input modalities and will be excluded.</p>}
    <button className="button primary" disabled={busy||!compatible.length||!modalities.length||(!target&&!name.trim())}>{target?"Create new dataset version":"Create dataset version"}</button>
  </form></section>;
}
