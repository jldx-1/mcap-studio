import { Fragment, useState } from "react";
import { Labels } from "../components/labels/Labels";
import { useTopicTranslations } from "../components/processing/TopicTranslations";
import type { WorkspaceController } from "../hooks/useWorkspace";
import type { Project } from "../types/models";
import { workspaceApi } from "../services/workspace";
import { can } from "../utils/permissions";
import { filename } from "../utils/recordings";

export function LabelsTranslationsPage({workspace,project}:{workspace:WorkspaceController;project:Project}) {
  const [selected,setSelected]=useState(workspace.recording?.uuid||project.mcaps[0]?.uuid||"");
  const recording=project.mcaps.find(item=>item.uuid===selected)||project.mcaps[0];
  return <div className="feature-stack">
    {can(project,workspace.user,"labels.manage")&&<Labels project={project} busy={!!workspace.busy} perform={workspace.perform} onProject={workspace.updateProject} onNotice={workspace.setNotice}/>}
    {can(project,workspace.user,"project.manage")&&<>
      <section className="panel"><div className="panel-heading"><div><h2>Translations</h2><p>Create reusable scripts for a topic and schema, preview their output, and apply them to a recording.</p></div></div><div className="collection-body"><label>Recording<select aria-label="Translation recording" disabled={!!workspace.busy} value={recording?.uuid||""} onChange={event=>setSelected(event.target.value)}>{!recording&&<option value="">No recordings yet</option>}{project.mcaps.map(item=><option key={item.uuid} value={item.uuid}>{filename(item.source_file)}</option>)}</select></label>{!recording&&<p className="field-help">Upload an MCAP in Recordings to create and preview translations.</p>}</div></section>
      {recording&&<TranslationTable key={`${project.uuid}/${recording.uuid}`} workspace={workspace} project={project} recordingId={recording.uuid}/>}
    </>}
  </div>;
}
function TranslationTable({workspace,project,recordingId}:{workspace:WorkspaceController;project:Project;recordingId:string}) {
  const {busy,perform,setNotice}=workspace;
  const translations=useTopicTranslations({project,recording:{uuid:recordingId},busy:!!busy,perform,enabled:true,onNotice:setNotice});
  return <section className="panel"><div className="panel-heading"><div><h2>Topic translations</h2><p>Presets match the topic, message type, and schema version.</p></div></div>
    {translations.loading?<p className="collection-body" role="status">Loading topics and translations…</p>:<div className="table-wrap"><table className="topic-table"><thead><tr><th>Topic</th><th>Message type</th><th>Translation script</th><th><span className="visually-hidden">Actions</span></th></tr></thead><tbody>{translations.topics.map(topic=>{
      const expanded=translations.editing&&translations.channel===topic.channel_id;
      return <Fragment key={topic.channel_id}><tr><td><strong>{topic.topic}</strong></td><td>{topic.source_message||topic.encoding||"JSON"}</td><td><select aria-label={`Saved translation for ${topic.topic}`} disabled={!!busy} value={translations.selected[topic.channel_id]||""} onChange={event=>translations.setSelected(values=>({...values,[topic.channel_id]:event.target.value}))}><option value="">No translation</option>{topic.choices.map(profile=><option key={profile.id} value={profile.id}>{profile.name}</option>)}</select></td><td><button className="button secondary small" aria-label={`Edit translation for ${topic.topic}`} aria-expanded={expanded} disabled={!!busy} onClick={()=>translations.edit(topic)}>{translations.selected[topic.channel_id]?"Inspect or edit":"Create or load"}</button></td></tr>{expanded&&<tr><td colSpan={4}>{translations.editor}</td></tr>}</Fragment>;
    })}</tbody></table>{!translations.topics.length&&<p className="collection-body muted">This recording has no topics.</p>}</div>}
    <div className="collection-body feature-stack">{translations.error&&!translations.editing&&<p className="inline-error" role="alert">{translations.error}</p>}<div className="button-row"><button className="button primary small" disabled={!!busy||translations.loading||!Object.keys(translations.selections).length} onClick={()=>void perform("Applying topic translations",async()=>{
      const output=await workspaceApi.applyTranslations(project.uuid,recordingId,translations.selections);
      workspace.setRecording(output);await workspace.updateProject(await workspaceApi.project(project.uuid));workspace.setPage("topics");setNotice("Translations applied. The recording is ready to label.");
    })}>Apply selected translations</button><span className="field-help">Applying scripts opens the updated recording’s topic table.</span></div></div>
  </section>;
}
