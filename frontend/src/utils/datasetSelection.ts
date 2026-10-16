import type { ApprovedSource, VersionSource } from "../types/datasetVersions";

export const revisionKey = (s: Pick<VersionSource, "project_uuid" | "task_id" | "revision">) => `${s.project_uuid}/${s.task_id}/${s.revision}`;
export const taskKey = (s: Pick<VersionSource, "project_uuid" | "task_id">) => `${s.project_uuid}/${s.task_id}`;

/** Keep one explicitly selected revision per task, including selections hidden by filters. */
export function selectRevisions(current: string[], next: string[], catalog: ApprovedSource[]) {
  const added = next.filter(key => !current.includes(key));
  const replaced = new Set(catalog.filter(s => added.includes(revisionKey(s))).map(taskKey));
  return [...new Set(next.filter(key => added.includes(key) || !replaced.has(key.slice(0, key.lastIndexOf("/")))))];
}

export function versionChanges(before: VersionSource[], after: ApprovedSource[]) {
  const old = new Map(before.map(s => [taskKey(s), s]));
  const next = new Map(after.map(s => [taskKey(s), s]));
  return {
    retained: after.filter(s => old.get(taskKey(s))?.revision === s.revision).length,
    added: after.filter(s => !old.has(taskKey(s))).length,
    updated: after.filter(s => old.has(taskKey(s)) && old.get(taskKey(s))?.revision !== s.revision).length,
    removed: before.filter(s => !next.has(taskKey(s))).length,
  };
}
