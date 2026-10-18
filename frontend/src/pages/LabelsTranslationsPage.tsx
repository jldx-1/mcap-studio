import { Fragment, useState } from "react";
import { Labels } from "../components/labels/Labels";
import { useTopicTranslations } from "../components/processing/TopicTranslations";
import type { WorkspaceController } from "../hooks/useWorkspace";
import type { Project } from "../types/models";
import { workspaceApi } from "../services/workspace";
import { can } from "../utils/permissions";
import { DefinitionTranslations } from "../components/processing/DefinitionTranslations";
import { filename } from "../utils/recordings";

export function LabelsTranslationsPage({workspace,project}:{workspace:WorkspaceController;project:Project}) {
  const labels=can(project,workspace.user,"labels.manage"),translate=can(project,workspace.user,"project.manage");
  const [tab,setTab]=useState(labels?"labels":"translations");
  const [source,setSource]=useState("seen"),[format,setFormat]=useState("");
  const [selected,setSelected]=useState(workspace.recording?.uuid||project.mcaps[0]?.uuid||"");
  const formats=[...new Set(project.mcaps.flatMap(item=>(item.topic_details||[]).map(topic=>`${topic.encoding}|${topic.schema_name}`)))].sort();
  const recordings=project.mcaps.filter(item=>!format||item.topic_details?.some(topic=>`${topic.encoding}|${topic.schema_name}`===format));
  const recording=recordings.find(item=>item.uuid===selected)||recordings[0];
  const tabs=[...(labels?[{id:"labels",name:"Labels"}]:[]),...(translate?[{id:"translations",name:"Translations"}]:[])];
  return <div className="feature-stack">
    <div role="tablist" aria-label="Labels and translations" className="workspace-tabs">{tabs.map((item,index)=><button key={item.id} id={`${item.id}-tab`} role="tab" aria-selected={tab===item.id} aria-controls={`${item.id}-panel`} tabIndex={tab===item.id?0:-1} onClick={()=>setTab(item.id)} onKeyDown={event=>{if(["ArrowLeft","ArrowRight","Home","End"].includes(event.key)){event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;setTab(tabs[next].id);document.getElementById(`${tabs[next].id}-tab`)?.focus();}}}>{item.name}</button>)}</div>
    {labels&&<div role="tabpanel" id="labels-panel" aria-labelledby="labels-tab" hidden={tab!=="labels"} className="labels-translations-panel"><Labels project={project} busy={!!workspace.busy} perform={workspace.perform} onProject={workspace.updateProject} onNotice={workspace.setNotice}/></div>}
    {translate&&<div role="tabpanel" id="translations-panel" aria-labelledby="translations-tab" hidden={tab!=="translations"} className="labels-translations-panel feature-stack">
      <section className="panel"><div className="panel-heading"><div><h2>Translation sources</h2><p>Map Protobuf or structured input fields into the internal CSV record format.</p></div></div><div className="collection-body approved-filter-controls"><label>Input source<select aria-label="Translation input source" value={source} onChange={event=>setSource(event.target.value)}><option value="seen">Detected encoding formats</option><option value="definitions">Load Protobuf definitions</option></select></label>
      {source==="seen"&&<><label>Encoding / message type<select aria-label="Detected encoding format" value={format} onChange={event=>setFormat(event.target.value)}><option value="">All detected formats</option>{formats.map(value=><option value={value} key={value}>{value.split("|").filter(Boolean).join(" · ")}</option>)}</select></label><label>Recording<select aria-label="Translation recording" disabled={!!workspace.busy} value={recording?.uuid||""} onChange={event=>setSelected(event.target.value)}>{!recording&&<option value="">No matching recordings</option>}{recordings.map(item=><option key={item.uuid} value={item.uuid}>{filename(item.source_file)}</option>)}</select></label></>}
      </div></section>
      {source==="definitions"?<DefinitionTranslations workspace={workspace} project={project}/>:recording?<TranslationTable key={`${project.uuid}/${recording.uuid}`} workspace={workspace} project={project} recordingId={recording.uuid} format={format}/>:<p className="field-help">Import an MCAP to discover its encodings and message schemas.</p>}
    </div>}
  </div>;
}

function TranslationTable({workspace,project,recordingId,format}:{workspace:WorkspaceController;project:Project;recordingId:string;format:string}) {
  const {busy,perform,setNotice}=workspace;
  const translations=useTopicTranslations({project,recording:{uuid:recordingId},busy:!!busy,perform,enabled:true,onNotice:setNotice});
  return <section className="panel"><div className="panel-heading"><div><h2>Topic translations</h2><p>Presets match the topic, message type, and schema version.</p></div></div>
    {translations.loading?<p className="collection-body" role="status">Loading topics and translations…</p>:<div className="table-wrap"><table className="topic-table"><thead><tr><th>Topic</th><th>Message type</th><th>Translation script</th><th><span className="visually-hidden">Actions</span></th></tr></thead><tbody>{translations.topics.filter(topic=>!format||`${topic.encoding}|${topic.source_message}`===format).map(topic=>{
      const expanded=translations.editing&&translations.channel===topic.channel_id;
      return <Fragment key={topic.channel_id}><tr><td><strong>{topic.topic}</strong></td><td>{topic.source_message||topic.encoding||"JSON"}</td><td><select aria-label={`Saved translation for ${topic.topic}`} disabled={!!busy} value={translations.selected[topic.channel_id]||""} onChange={event=>translations.setSelected(values=>({...values,[topic.channel_id]:event.target.value}))}><option value="">No translation</option>{topic.choices.map(profile=><option key={profile.id} value={profile.id}>{profile.name}</option>)}</select></td><td><button className="button secondary small" aria-label={`Edit translation for ${topic.topic}`} aria-expanded={expanded} disabled={!!busy} onClick={()=>translations.edit(topic)}>{translations.selected[topic.channel_id]?"Inspect or edit":"Create or load"}</button></td></tr>{expanded&&<tr><td colSpan={4}>{translations.editor}</td></tr>}</Fragment>;
    })}</tbody></table>{!translations.topics.length&&<p className="collection-body muted">This recording has no topics.</p>}</div>}
    <div className="collection-body feature-stack">{translations.error&&!translations.editing&&<p className="inline-error" role="alert">{translations.error}</p>}<div className="button-row"><button className="button primary small" disabled={!!busy||translations.loading||!Object.keys(translations.selections).length} onClick={()=>void perform("Applying topic translations",async()=>{
      const output=await workspaceApi.applyTranslations(project.uuid,recordingId,translations.selections);
      workspace.setRecording(output);await workspace.updateProject(await workspaceApi.project(project.uuid));workspace.setPage("topics");setNotice("Translations applied. The recording is ready to label.");
    })}>Apply selected translations</button><span className="field-help">Applying scripts opens the updated recording’s topic table.</span></div></div>
  </section>;
}
