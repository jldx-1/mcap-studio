import { useEffect,useState } from "react";
import type { DatasetWorkspaceProps } from "../../hooks/useDatasetWorkspace";
import { useApprovedData } from "../../hooks/useApprovedData";
import { api } from "../../services/http";
import { useAuth } from "../../state/AuthContext";
import type { DatasetVersion } from "../../types/datasetVersions";
import type { Dataset } from "../../types/models";
import { can } from "../../utils/permissions";
import { ApprovedDataBrowser,selectedApprovedSources } from "./ApprovedDataBrowser";
import { DatasetCreation } from "./DatasetCreation";
import { DatasetVersionDetails } from "./DatasetVersionDetails";

export function DatasetWorkspace({project,mode,onModeChange,busy,perform,onProject,onNotice}:DatasetWorkspaceProps){
  const {user}=useAuth();const [error,setError]=useState("");const data=useApprovedData(project.uuid,setError);
  const [tab,setTab]=useState<"available"|"datasets">(mode==="new"||!project.datasets.some((d)=>d.kind==="approved-dataset")?"available":"datasets");
  const [selected,setSelected]=useState<string[]>([]),[creating,setCreating]=useState(mode==="new"),[target,setTarget]=useState<Dataset|null>(null);
  const [dataset,setDataset]=useState<Dataset|null>(null),[version,setVersion]=useState<DatasetVersion|null>(null);
  useEffect(()=>{if(mode==="new"||mode==="add"){setTab("available");setCreating(true);}},[mode]);
  const ready=selectedApprovedSources(data.all,selected);
  const open=(item:Dataset)=>void perform("Opening dataset version",async()=>{setDataset(item);setVersion(await api<DatasetVersion>(`/projects/${project.uuid}/datasets/${item.uuid}/versions/${item.latest_version}`));setCreating(false);setTab("datasets");onModeChange("view");});
  return <div className="feature-stack">{error&&<p className="inline-error" role="alert">{error}</p>}<div className="dataset-section-tabs"><button className={`button ${tab==="available"?"primary":"secondary"}`} onClick={()=>{setTab("available");onModeChange("new");}}>Approved data</button><button className={`button ${tab==="datasets"?"primary":"secondary"}`} onClick={()=>{setTab("datasets");onModeChange("load");}}>Dataset versions</button></div>
    {tab==="available"&&<><ApprovedDataBrowser data={data} selected={selected} onSelected={setSelected} busy={busy}/>{can(project,user,"datasets.generate")&&<div className="button-row"><button className="button primary" disabled={busy||!ready.length} onClick={()=>{setCreating(true);setTarget(null);}}>Create dataset from selected revisions</button></div>}{creating&&ready.length>0&&<DatasetCreation key={target?.uuid||"new"} project={project} target={target} sources={ready} data={data} busy={busy} perform={perform} onProject={onProject} onCreated={(item,v)=>{setDataset(item);setVersion(v);setCreating(false);setTarget(null);setTab("datasets");onModeChange("view");onNotice(`${item.name} version ${v.number} created from approved revisions.`);}}/>}</>}
    {tab==="datasets"&&<>{!dataset||!version?<section className="panel"><div className="panel-heading"><div><h2>Reproducible datasets</h2><p>Each version keeps its approved inputs. New approved data is available separately.</p></div></div>{project.datasets.filter((d)=>d.kind==="approved-dataset").map((item)=><button key={item.uuid} className="recording-row" disabled={busy} onClick={()=>open(item)}><div><strong>{item.name}</strong><span>Version {item.latest_version} · {item.entry_count.toLocaleString()} samples · {item.versions?.length||0} preserved versions</span></div></button>)}{!project.datasets.some((d)=>d.kind==="approved-dataset")&&<p className="collection-body">Select approved revisions in Approved data to create the first training dataset.</p>}{project.datasets.some((d)=>d.kind!=="approved-dataset")&&<details className="collection-body"><summary>Working label tables and legacy datasets</summary><p className="field-help">These existing files remain available on disk. Production dataset versions are created from approved revision snapshots.</p>{project.datasets.filter((d)=>d.kind!=="approved-dataset").map((d)=><p key={d.uuid}>{d.name} · {d.entry_count.toLocaleString()} rows · {d.source_recording_uuid?"working CSV label table":"legacy membership"}</p>)}</details>}</section>:<><div className="button-row"><button className="button secondary small" onClick={()=>{setDataset(null);setVersion(null);onModeChange("load");}}>All datasets</button>{data.all.some((s)=>version.source_revisions.some((old)=>old.task_id===s.task_id&&old.project_uuid===s.project_uuid&&s.revision>old.revision))&&<span className="pill blue">New approved revisions available · create a new version to use them</span>}</div><DatasetVersionDetails project={project} dataset={project.datasets.find((d)=>d.uuid===dataset.uuid)||dataset} version={version} onVersion={setVersion} busy={busy} perform={perform} onNotice={onNotice} onNewVersion={()=>{setTarget(dataset);setCreating(true);setTab("available");onModeChange("add");}}/></>}</>}
  </div>;
}
