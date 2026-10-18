import { useEffect, useRef, useState } from "react";
import type { WorkspaceController } from "../../hooks/useWorkspace";
import type { Project } from "../../types/models";
import { api, post } from "../../services/http";
interface Definition {id:string;message:string;fields:{name:string;type:string;repeated:boolean}[]}
interface Preview {source:unknown;columns:string[];row:Record<string,unknown>}
const readBase64=(file:File)=>new Promise<string>((resolve,reject)=>{const reader=new FileReader();reader.onerror=()=>reject(new Error(`Cannot read ${file.name}`));reader.onload=()=>resolve(String(reader.result).split(",")[1]);reader.readAsDataURL(file);});
export function DefinitionTranslations({workspace,project}:{workspace:WorkspaceController;project:Project}) {
  const [definitions,setDefinitions]=useState<Definition[]>([]),[selected,setSelected]=useState("");
  const [name,setName]=useState(""),[script,setScript]=useState("* -> *"),[sample,setSample]=useState("{}"),[sampleBytes,setSampleBytes]=useState<string|null>(null),[preview,setPreview]=useState<Preview|null>(null),[error,setError]=useState("");
  const files=useRef<HTMLInputElement>(null),binary=useRef<HTMLInputElement>(null);
  const base=`/projects/${project.uuid}/translation-definitions`;
  useEffect(()=>{let active=true;void api<{definitions:Definition[]}>(base).then(value=>{if(active)setDefinitions(value.definitions);}).catch(error=>{if(active)setError(String(error));});return()=>{active=false;};},[base]);
  const definition=definitions.find(item=>`${item.id}:${item.message}`===selected)||definitions[0];
  const request=()=>({definition_id:definition?.id,message:definition?.message,script,...(sampleBytes?{sample_base64:sampleBytes}:{sample:JSON.parse(sample)})});
  const perform=(label:string,run:()=>Promise<void>)=>void workspace.perform(label,async()=>{try{await run();setError("");}catch(error){setError(error instanceof Error?error.message:String(error));throw error;}});
  const invalidate=()=>setPreview(null);
  return <section className="panel"><div className="panel-heading"><div><h2>Protobuf → CSV translation</h2><p>Load .proto files with their imports, or a compiled FileDescriptorSet (.desc, .pb, .bin). Definitions stay in this project.</p></div><button className="button secondary small" disabled={!!workspace.busy} onClick={()=>files.current?.click()}>Load Protobuf definitions</button></div>
    <input ref={files} type="file" multiple accept=".proto,.desc,.pb,.bin" className="visually-hidden" aria-label="Protobuf definition files" onChange={event=>{const selected=Array.from(event.target.files||[]);event.target.value="";if(!selected.length)return;perform("Loading Protobuf definitions",async()=>{
      if(selected.reduce((total,file)=>total+file.size,0)>8*1024*1024)throw new Error("Definitions must total at most 8 MiB.");
      const result=await post<{definitions:Definition[]}>(base,{action:"upload",files:await Promise.all(selected.map(async file=>({name:file.webkitRelativePath||file.name,data_base64:await readBase64(file)})))});
      setDefinitions(values=>[...new Map([...values,...result.definitions].map(item=>[`${item.id}:${item.message}`,item])).values()]);const first=result.definitions[0];if(first)setSelected(`${first.id}:${first.message}`);invalidate();
    });}}/>
    <div className="collection-body feature-stack">{error&&<p className="inline-error" role="alert">{error}</p>}
      {!definition?<p className="muted">Load a definition to choose its message type and create a reusable CSV mapping.</p>:<>
        <label>Input message type<select aria-label="Protobuf input message" disabled={!!workspace.busy} value={`${definition.id}:${definition.message}`} onChange={event=>{setSelected(event.target.value);setSampleBytes(null);setSample("{}");invalidate();}}>{definitions.map(item=><option key={`${item.id}:${item.message}`} value={`${item.id}:${item.message}`}>{item.message} · {item.id.slice(0,8)}</option>)}</select></label>
        <details><summary>Message fields ({definition.fields.length})</summary><ul>{definition.fields.map(field=><li key={field.name}><code>{field.name}</code> · {field.type}{field.repeated?"[]":""}</li>)}</ul></details>
        <label>Translation name<input aria-label="Definition translation name" value={name} maxLength={120} disabled={!!workspace.busy} onChange={event=>setName(event.target.value)}/></label>
        <div className="translation-preview-grid"><label>Sample input (Protobuf JSON)<textarea aria-label="Protobuf sample JSON" rows={8} disabled={!!workspace.busy||!!sampleBytes} value={sample} onChange={event=>{setSample(event.target.value);invalidate();}}/></label><label>ETL script<textarea aria-label="Protobuf ETL script" rows={8} disabled={!!workspace.busy} value={script} onChange={event=>{setScript(event.target.value);invalidate();}}/></label></div>
        <div className="button-row"><button className="button secondary small" disabled={!!workspace.busy} onClick={()=>binary.current?.click()}>Load encoded sample</button>{sampleBytes&&<button className="text-button" onClick={()=>{setSampleBytes(null);invalidate();}}>Use JSON sample</button>}<button className="button secondary small" disabled={!!workspace.busy||!script.trim()} onClick={()=>perform("Previewing CSV mapping",async()=>setPreview(await post<Preview>(base,{action:"preview",...request()})))}>Preview CSV record</button><button className="button primary small" disabled={!!workspace.busy||!name.trim()||!script.trim()} onClick={()=>perform("Saving CSV mapping",async()=>{await post(base,{action:"save",name:name.trim(),...request()});workspace.setNotice("Translation saved for matching Protobuf topics and schema versions.");})}>Save CSV translation</button></div>
        <input type="file" ref={binary} className="visually-hidden" aria-label="Encoded Protobuf sample" onChange={event=>{const file=event.target.files?.[0];event.target.value="";if(file)perform("Reading Protobuf sample",async()=>{if(file.size>8*1024*1024)throw new Error("Sample must be at most 8 MiB.");setSampleBytes(await readBase64(file));invalidate();});}}/>
        {preview&&<div><h3>CSV output</h3><div className="table-wrap"><table><thead><tr>{preview.columns.map(column=><th key={column}>{column}</th>)}</tr></thead><tbody><tr>{preview.columns.map(column=><td key={column}>{typeof preview.row[column]==="object"?JSON.stringify(preview.row[column]):String(preview.row[column]??"")}</td>)}</tr></tbody></table></div><details><summary>Decoded input</summary><pre>{JSON.stringify(preview.source,null,2)}</pre></details></div>}
        <p className="field-help">Mappings save against this message type and schema. Matching topics offer them under detected formats; applying a mapping makes structured rows available to the CSV workspace.</p>
      </>}
    </div>
  </section>;
}
