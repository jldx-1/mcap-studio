import { useEffect,useState } from "react";
import { api,put } from "../../services/http";
import type { Perform } from "../../types/workspace";

interface Assignment{uuid:string;name:string;assigned:boolean}
export function UserProjectAssignments({userId,busy,perform}:{userId:string;busy:boolean;perform:Perform}){
  const [projects,setProjects]=useState<Assignment[]>([]);const [loading,setLoading]=useState(true);
  const base=`/accounts/users/${userId}/projects`;
  useEffect(()=>{let active=true;setLoading(true);void api<Assignment[]>(base).then((p)=>{if(active)setProjects(p);}).catch((e)=>void perform("Loading project access",async()=>{throw e;})).finally(()=>{if(active)setLoading(false);});return()=>{active=false;};},[base,perform]);
  return <div className="user-project-assignment"><h3>Project access</h3><p className="field-help">Account permissions apply only to assigned projects. Existing project-specific grants are preserved.</p>{loading?<p role="status">Loading projects…</p>:<><fieldset disabled={busy}><legend>Assigned projects</legend>{projects.map((p)=><label className="check-label" key={p.uuid}><input type="checkbox" checked={p.assigned} onChange={(e)=>setProjects((current)=>current.map((item)=>item.uuid===p.uuid?{...item,assigned:e.target.checked}:item))}/>{p.name}</label>)}{!projects.length&&<p>No projects are available for you to assign.</p>}</fieldset><button type="button" className="button secondary" disabled={busy||!projects.length} onClick={()=>void perform("Saving project access",async()=>setProjects(await put<Assignment[]>(base,{project_ids:projects.filter((p)=>p.assigned).map((p)=>p.uuid)})))}>Save project access</button></>}
  </div>;
}
