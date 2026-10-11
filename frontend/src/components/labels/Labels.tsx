import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { post, put } from "../../services/http";
import type { Project } from "../../types/models";
import type { Perform } from "../../types/workspace";

export function Labels({ project, busy, perform, onProject, onNotice }: {
  project: Project; busy: boolean; perform: Perform; onProject: (project: Project) => Promise<void>; onNotice: (notice: string) => void;
}) {
  const [listName, setListName] = useState("");
  const [names, setNames] = useState<Record<string, string>>({});
  const base = `/projects/${project.uuid}`;
  const updateList = (list: Project["label_lists"][number], ids: string[], message: string) =>
    void perform("Updating label list", async () => {
      await onProject(await put<Project>(`${base}/label-lists/${list.uuid}`, { label_ids: ids, expected_label_ids: list.label_ids }));
      onNotice(message);
    });
  return <div className="labels-manager">
    <section className="panel">
      <div className="panel-heading"><div><h2>Label lists</h2><p>Create reusable groups, then add individual labels inside each list.</p></div><span className="pill">{project.label_lists.length} lists</span></div>
      <div className="collection-body">
        <form className="inline-form" onSubmit={(event) => {
          event.preventDefault(); void perform("Creating label list", async () => {
            await onProject(await post<Project>(`${base}/label-lists`, { name: listName.trim(), label_ids: [] })); setListName(""); onNotice("Label list created.");
          });
        }}><input aria-label="New label list name" placeholder="e.g. Vehicle Types" value={listName} onChange={(event) => setListName(event.target.value)} required /><button className="button primary" disabled={busy || !listName.trim()}><Plus size={17} /> Create label list</button></form>
      </div>
    </section>
    {project.label_lists.map((list) => <section className="panel" key={list.uuid}>
      <div className="panel-heading"><h2>{list.name}</h2><span className="pill">{list.label_ids.length} labels</span></div>
      <div className="collection-body">
        <ol className="ordered-label-list">
          {list.label_ids.map((id, index) => {
            const name = project.labels.find((label) => label.uuid === id)?.name || id;
            const move = (direction: number) => {
              const ids = [...list.label_ids];
              [ids[index], ids[index + direction]] = [ids[index + direction], ids[index]];
              updateList(list, ids, `Label order updated in ${list.name}.`);
            };
            return <li key={id}>
              <span className="label-position" aria-label={`Position ${index + 1}`}>{index + 1}</span>
              <strong>{name}</strong>
              <div className="button-row">
                <button className="icon-button" disabled={busy || index === 0} aria-label={`Move ${name} up in ${list.name}`} title="Move up" onClick={() => move(-1)}><ArrowUp size={16} /></button>
                <button className="icon-button" disabled={busy || index === list.label_ids.length - 1} aria-label={`Move ${name} down in ${list.name}`} title="Move down" onClick={() => move(1)}><ArrowDown size={16} /></button>
                <button className="icon-button danger-text" disabled={busy} aria-label={`Remove ${name} from ${list.name}`} title="Remove from list" onClick={() => updateList(list, list.label_ids.filter((label) => label !== id), `${name} removed from ${list.name}. Saved annotations are preserved.`)}><Trash2 size={16} /></button>
              </div>
            </li>;
          })}
        </ol>
        {!!list.label_ids.length && <p className="field-help">Numbers show list order. Reordering or removing labels preserves saved annotations and class IDs.</p>}
        {!list.label_ids.length && <p className="muted">This list has no labels yet.</p>}
        <form className="inline-form" onSubmit={(event) => {
          event.preventDefault(); void perform("Adding label to list", async () => {
            await onProject(await post<Project>(`${base}/label-lists/${list.uuid}/labels`, { name: (names[list.uuid] || "").trim() })); setNames((current) => ({ ...current, [list.uuid]: "" })); onNotice(`Label added to ${list.name}.`);
          });
        }}><input aria-label={`New label in ${list.name}`} placeholder="e.g. Car" value={names[list.uuid] || ""} onChange={(event) => setNames((current) => ({ ...current, [list.uuid]: event.target.value }))} required /><button className="button secondary" disabled={busy || !names[list.uuid]?.trim()}><Plus size={16} /> Add label</button></form>
      </div>
    </section>)}
  </div>;
}
