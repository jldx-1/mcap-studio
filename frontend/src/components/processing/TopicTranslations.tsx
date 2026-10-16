import { useEffect, useRef, useState } from "react";
import { api, post } from "../../services/http";
import type { Project, Recording } from "../../types/models";
import type { Perform } from "../../types/workspace";

interface Translation {id:string;name:string;script:string;source_message:string;source_schema_sha256:string;topic?:string;[key:string]:unknown}
interface TranslationTopic {channel_id:string;topic:string;source_message:string;source_schema_sha256:string;choices:Translation[]}
interface Preview {source:unknown;translated:unknown}
interface LiveOutput {channel:string;script:string;value:unknown}
interface PreviewError {channel:string;script:string;message:string}

export function useTopicTranslations({project,recording,busy,perform,enabled,onNotice}: {
  project:Project;recording:Recording;busy:boolean;perform:Perform;
  enabled:boolean;onNotice:(message:string)=>void;
}) {
  const [topics,setTopics] = useState<TranslationTopic[]>([]);
  const [selected,setSelected] = useState<Record<string,string>>({});
  const [editing,setEditing] = useState(false),[channel,setChannel] = useState("");
  const [name,setName] = useState(""),[script,setScript] = useState("* -> *\n");
  const [fileProfile,setFileProfile] = useState<Record<string,unknown>>({});
  const [source,setSource] = useState<{channel:string;value:unknown}|null>(null);
  const [output,setOutput] = useState<LiveOutput|null>(null);
  const [sourceError,setSourceError] = useState("");
  const [previewError,setPreviewError] = useState<PreviewError|null>(null);
  const [error,setError] = useState(""),[loading,setLoading] = useState(true);
  const fileInput = useRef<HTMLInputElement>(null);
  const base = `/projects/${project.uuid}/recordings/${recording.uuid}/translations`;
  useEffect(()=>{
    if(!enabled)return;
    let active=true;
    void api<{topics:TranslationTopic[]}>(base).then(result=>{
      if(!active)return;setTopics(result.topics);setChannel(result.topics[0]?.channel_id||"");
      // Suggest an unambiguous known preset, while leaving application explicit.
      setSelected(Object.fromEntries(result.topics.filter(topic=>topic.choices.length===1).map(topic=>[topic.channel_id,topic.choices[0].id])));
    }).catch(error=>{if(active)setError(String(error));}).finally(()=>{if(active)setLoading(false);});
    return()=>{active=false;};
  },[base,enabled]);
  // Load the original independently so it remains visible even for an invalid script.
  useEffect(()=>{
    if(!enabled||!editing||!channel)return;
    const controller=new AbortController();
    let active=true;
    setSource(null);setSourceError("");
    void api<Preview>(base+"/preview",{
      method:"POST",body:JSON.stringify({channel_id:channel,script:"* -> *"}),signal:controller.signal,
    }).then(result=>{if(active)setSource({channel,value:result.source});})
      .catch(error=>{if(active)setSourceError(error instanceof Error?error.message:String(error));});
    return()=>{active=false;controller.abort();};
  },[base,channel,editing,enabled]);
  // Debounce typing and discard responses from older scripts or a closed topic row.
  useEffect(()=>{
    if(!enabled||!editing||!channel||!script.trim())return;
    const controller=new AbortController();
    let active=true;
    const timer=setTimeout(()=>{
      void api<Preview>(base+"/preview",{
        method:"POST",body:JSON.stringify({channel_id:channel,script}),signal:controller.signal,
      }).then(result=>{
        if(active){setSource({channel,value:result.source});setSourceError("");setOutput({channel,script,value:result.translated});setPreviewError(null);}
      }).catch(error=>{
        if(active)setPreviewError({channel,script,message:error instanceof Error?error.message:String(error)});
      });
    },350);
    return()=>{active=false;clearTimeout(timer);controller.abort();};
  },[base,channel,script,editing,enabled]);
  const currentSource=source?.channel===channel?source:null;
  const currentOutput=output?.channel===channel&&output.script===script?output:null;
  const currentPreviewError=previewError?.channel===channel&&previewError.script===script?previewError.message:"";
  const topic = topics.find(topic=>topic.channel_id===channel);
  const selections = Object.fromEntries(Object.entries(selected).filter(([,id])=>!!id));
  const edit = (topic:TranslationTopic) => {
    if(editing&&channel===topic.channel_id)return;
    const profile = topic.choices.find(choice=>choice.id===selected[topic.channel_id]);
    setChannel(topic.channel_id);setName(profile?.name||"");setScript(profile?.script||"* -> *\n");setFileProfile(profile||{});setSource(null);setOutput(null);setPreviewError(null);setSourceError("");setEditing(true);
  };
  const editor = editing&&<form className="account-form topic-translation-editor" onSubmit={event=>{event.preventDefault();void perform("Saving topic translation",async()=>{
      const saved = await post<Translation>(base,{channel_id:channel,translation:{...fileProfile,name:name.trim(),script}});
      const result = await api<{topics:TranslationTopic[]}>(base);setTopics(result.topics);setSelected(current=>({...current,[channel]:saved.id}));setFileProfile(saved);setEditing(false);onNotice("Translation saved for this topic and schema version.");
    });}}>
      <h3>Translation for {topic?.topic}</h3>
      {error&&<p className="inline-error" role="alert">{error}</p>}
      <label>Translation name<input required maxLength={120} value={name} disabled={busy} onChange={event=>setName(event.target.value)} placeholder="e.g. Motor units to SI"/></label>
      <div className="button-row"><button type="button" className="button secondary small" disabled={busy||!topic} onClick={()=>fileInput.current?.click()}>Load translation file</button>{topic&&<span className="field-help">{topic.source_message||"JSON topic"} · Schema {topic.source_schema_sha256.slice(0,12)}</span>}</div>
      <input ref={fileInput} type="file" className="visually-hidden" accept=".json,.txt" aria-label="Translation file" disabled={busy} onChange={event=>{
        const file=event.target.files?.[0];event.target.value="";if(!file)return;
        void (async()=>{try{
          if(file.size>256*1024)throw new Error("Translation files must be smaller than 256 KiB.");
          const text=await file.text();
          const profile:Record<string,unknown>=file.name.toLowerCase().endsWith(".json")?JSON.parse(text):{name:file.name.replace(/\.[^.]+$/,""),script:text};
          if(typeof profile!=="object"||profile===null||Array.isArray(profile))throw new Error("Translation file must be a JSON object or script text.");
          if(topic&&((profile.topic!==undefined&&profile.topic!==topic.topic)||(profile.source_message!==undefined&&profile.source_message!==topic.source_message)||(profile.source_schema_sha256!==undefined&&profile.source_schema_sha256!==topic.source_schema_sha256)))throw new Error("This translation belongs to a different topic, message type, or schema version.");
          const mappings=profile.mappings as {target:string;expression:string}[]|undefined;
          const textScript=typeof profile.script==="string"?profile.script:Array.isArray(mappings)?mappings.map(mapping=>`${mapping.target} <- ${mapping.expression}`).join("\n"):null;
          if(textScript===null)throw new Error("Translation file needs script text or mappings.");
          setFileProfile(profile);setName(typeof profile.name==="string"?profile.name:file.name);setScript(textScript);setError("");
        }catch(error){setError(error instanceof Error?error.message:String(error));}})();
      }}/>
      <div className="translation-preview-grid">
        <div className="translation-preview-pane">
          <h3>Current input</h3>
          <p className="field-help">Recorded sample from this topic</p>
          {sourceError?<p className="inline-error" role="alert">{sourceError}</p>:currentSource?<pre>{JSON.stringify(currentSource.value,null,2)}</pre>:<p className="field-help" role="status">Loading recorded sample…</p>}
        </div>
        <div className="translation-preview-pane">
          <label>Translation script<textarea aria-label="Topic translation script" rows={12} value={script} disabled={busy} onChange={event=>setScript(event.target.value)}/></label>
        </div>
        <div className="translation-preview-pane">
          <h3>Output</h3>
          <p className="field-help">Updates automatically as you edit</p>
          {currentPreviewError?<p className="inline-error" role="alert">{currentPreviewError}</p>:currentOutput?<pre>{JSON.stringify(currentOutput.value,null,2)}</pre>:<p className="field-help" role="status">{script.trim()?"Updating preview…":"Enter a translation script to see output."}</p>}
        </div>
      </div>
      <p className="field-help">Use * -&gt; * to keep unmapped fields, position.x -&gt; x_mm * 1000 to rename and convert, or total &lt;- max(source.values). Array indexing and abs/round/min/max/sqrt are supported.</p>
      <div className="button-row"><button className="button primary small" disabled={busy||!name.trim()||!script.trim()}>Save translation</button><button type="button" className="text-button" disabled={busy} onClick={()=>setEditing(false)}>Cancel</button></div>
    </form>;
  return {topics,selected,setSelected,edit,channel,editing,editor,error,loading,selections};
}
