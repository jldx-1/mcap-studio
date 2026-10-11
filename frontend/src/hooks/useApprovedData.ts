import { useCallback,useEffect,useMemo,useState } from "react";
import { api } from "../services/http";
import type { ApprovedCatalog,ApprovedSource } from "../types/datasetVersions";

export function sourceKey(source:ApprovedSource){return `${source.project_uuid}/${source.task_id}/${source.revision}`;}
export function useApprovedData(project:string,onError:(message:string)=>void){
  const [catalog,setCatalog]=useState<ApprovedCatalog>({projects:[],sources:[]});const [loading,setLoading]=useState(true);
  const [projectFilter,setProjectFilter]=useState(project),[labels,setLabels]=useState<string[]>([]),[modalities,setModalities]=useState<string[]>([]);
  const [search,setSearch]=useState(""),[annotator,setAnnotator]=useState(""),[reviewer,setReviewer]=useState(""),[type,setType]=useState(""),[revision,setRevision]=useState("");
  const [latest,setLatest]=useState(true),[sort,setSort]=useState("approved_at"),[descending,setDescending]=useState(true);
  const refresh=useCallback(async()=>{try{setCatalog(await api<ApprovedCatalog>("/approved-data"));}catch(err){onError(String(err));}finally{setLoading(false);}},[onError]);
  useEffect(()=>{void refresh();const timer=window.setInterval(()=>{if(document.visibilityState==="visible")void refresh();},15000);return()=>window.clearInterval(timer);},[refresh]);
  const all=useMemo(()=>{const sources=projectFilter?catalog.sources.filter((s)=>s.project_uuid===projectFilter):catalog.sources;if(!latest)return sources;const chosen=new Map<string,ApprovedSource>();for(const source of sources){const key=`${source.project_uuid}/${source.task_id}`;if(!chosen.has(key)||chosen.get(key)!.revision<source.revision)chosen.set(key,source);}return [...chosen.values()];},[catalog,projectFilter,latest]);
  const labelOptions=[...new Map(all.filter((s)=>!modalities.length||modalities.some((m)=>s.modalities.some((i)=>i.id===m))).flatMap((s)=>s.labels).map((l)=>[l.id,l])).values()].sort((a,b)=>a.name.localeCompare(b.name));
  const modalityOptions=[...new Map(all.filter((s)=>!labels.length||labels.some((l)=>s.labels.some((i)=>i.id===l))).flatMap((s)=>s.modalities).map((m)=>[m.id,{id:m.id,name:m.name}])).values()].sort((a,b)=>a.name.localeCompare(b.name));
  const filtered=all.filter((s)=>(!labels.length||labels.some((l)=>s.labels.some((i)=>i.id===l)))&&(!modalities.length||modalities.every((m)=>s.modalities.some((i)=>i.id===m)))&&(!search||`${s.source_file} ${s.project_name}`.toLowerCase().includes(search.toLowerCase()))&&(!annotator||s.annotator===annotator)&&(!reviewer||s.reviewer===reviewer)&&(!type||s.annotation_types.includes(type))&&(!revision||s.revision===Number(revision))).sort((a,b)=>{let value=0;if(sort==="samples")value=a.sample_count-b.sample_count;else if(sort==="revision")value=a.revision-b.revision;else if(sort==="label")value=(a.labels[0]?.name||"").localeCompare(b.labels[0]?.name||"");else if(sort==="modality")value=(a.modalities[0]?.name||"").localeCompare(b.modalities[0]?.name||"");else if(sort==="mcap")value=a.source_file.localeCompare(b.source_file);else if(sort==="project")value=a.project_name.localeCompare(b.project_name);else value=a.approved_at.localeCompare(b.approved_at);return descending?-value:value;});
  return{catalog,loading,refresh,all,filtered,labelOptions,modalityOptions,projectFilter,setProjectFilter,labels,setLabels,modalities,setModalities,search,setSearch,annotator,setAnnotator,reviewer,setReviewer,type,setType,revision,setRevision,latest,setLatest,sort,setSort,descending,setDescending};
}
