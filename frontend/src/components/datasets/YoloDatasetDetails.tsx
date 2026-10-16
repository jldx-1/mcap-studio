import { useEffect, useState } from "react";
import { api } from "../../services/http";
import { useAuth } from "../../state/AuthContext";
import type { Dataset, Project } from "../../types/models";
import { can } from "../../utils/permissions";

interface YoloSample {
  index: number; source_file: string; split: string; width: number; height: number;
  boxes: {class_id:number;x_center:number;y_center:number;width:number;height:number}[];
}
const colors = ["#4592ff","#20b78a","#ff8b40","#b078ff","#ec5890"];

export function YoloDatasetDetails({project,dataset,busy,onError}: {project:Project;dataset:Dataset;busy:boolean;onError:(message:string)=>void}) {
  const {user} = useAuth();
  const [page,setPage] = useState(0),[samples,setSamples] = useState<YoloSample[]>([]),[selected,setSelected] = useState<YoloSample|null>(null),[loading,setLoading] = useState(false);
  const base = `/projects/${project.uuid}/datasets/${dataset.uuid}`;
  useEffect(()=>{
    let active = true;setLoading(true);setSelected(null);
    void api<{samples:YoloSample[]}>(`${base}/samples?offset=${page*24}&limit=24`).then(result=>{if(active)setSamples(result.samples);}).catch(error=>{if(active)onError(String(error));}).finally(()=>{if(active)setLoading(false);});
    return ()=>{active=false;};
  },[base,page,onError]);
  return <div className="feature-stack"><section className="panel"><div className="panel-heading"><div><h2>{dataset.name}</h2><p>Imported YOLO · {dataset.entry_count.toLocaleString()} frames · {dataset.box_count?.toLocaleString()} boxes · {dataset.negative_count?.toLocaleString()} negative frames</p></div>{can(project,user,"datasets.export")&&<a className="button secondary small" href={`/api${base}/archive`} download>Download YOLO ZIP</a>}</div><div className="collection-body"><div className="button-row">{Object.entries(dataset.split_counts||{}).map(([name,count])=><span className="pill" key={name}>{name}: {count.toLocaleString()}</span>)}</div><p>Classes: {dataset.class_names?.join(", ")||"No categorical labels"}</p></div></section>
    {selected&&<section className="panel"><div className="panel-heading"><div><h2>{selected.source_file}</h2><p>{selected.split} · {selected.width} × {selected.height} · {selected.boxes.length} boxes</p></div><button className="text-button" onClick={()=>setSelected(null)}>Close frame</button></div><div className="collection-body"><svg className="yolo-frame-preview" viewBox={`0 0 ${selected.width} ${selected.height}`} role="img" aria-label={`YOLO annotations for ${selected.source_file}`}><image width={selected.width} height={selected.height} href={`/api${base}/images/${selected.index}`}/>{selected.boxes.map((box,index)=><rect key={index} x={(box.x_center-box.width/2)*selected.width} y={(box.y_center-box.height/2)*selected.height} width={box.width*selected.width} height={box.height*selected.height} fill="none" stroke={colors[box.class_id%colors.length]} strokeWidth={2} vectorEffect="non-scaling-stroke"><title>{dataset.class_names?.[box.class_id]||`Class ${box.class_id}`}</title></rect>)}</svg><p>{[...new Set(selected.boxes.map(box=>dataset.class_names?.[box.class_id]||`Class ${box.class_id}`))].join(", ")||"Negative frame · no objects labelled"}</p></div></section>}
    <section className="panel"><div className="panel-heading"><h2>Frames and labels</h2><span className="pill">{dataset.entry_count.toLocaleString()} frames</span></div>{loading?<p className="collection-body" role="status">Loading frames…</p>:<div className="yolo-frame-grid collection-body">{samples.map(sample=><button className="yolo-frame-card" key={sample.index} onClick={()=>setSelected(sample)}><img loading="lazy" src={`/api${base}/images/${sample.index}`} alt={sample.source_file}/><strong>{sample.source_file}</strong><span>{sample.split} · {sample.boxes.length} boxes</span></button>)}</div>}<div className="dataset-selection-footer button-row"><button className="button secondary small" disabled={busy||loading||page===0} onClick={()=>setPage(value=>value-1)}>Previous frames</button><span>{dataset.entry_count?page*24+1:0}–{Math.min((page+1)*24,dataset.entry_count)} of {dataset.entry_count}</span><button className="button secondary small" disabled={busy||loading||(page+1)*24>=dataset.entry_count} onClick={()=>setPage(value=>value+1)}>Next frames</button></div></section>
  </div>;
}
