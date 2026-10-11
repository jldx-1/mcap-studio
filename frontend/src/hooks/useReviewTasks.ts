import { useCallback, useEffect, useState } from "react";
import { api, post } from "../services/http";
import type { ReviewTask } from "../types/reviews";

export function useReviewTasks(project: string, onError: (message: string) => void, enabled = true) {
  const [tasks,setTasks] = useState<ReviewTask[]>([]);
  const [loading,setLoading] = useState(true);
  const base = `/projects/${project}/tasks`;
  const refresh = useCallback(async () => { setTasks(await api<ReviewTask[]>(base));setLoading(false); },[base]);
  useEffect(() => {
    if(!enabled) { setLoading(false);return; }
    let active = true;
    const load = async () => { try { const values = await api<ReviewTask[]>(base);if(active) { setTasks(values);setLoading(false); } } catch (err) { if(active) { onError(String(err));setLoading(false); } } };
    void load();const timer = window.setInterval(() => { if(document.visibilityState === "visible") void load(); },15000);
    return () => { active=false;window.clearInterval(timer); };
  },[base,onError,enabled]);
  const action = async (task: ReviewTask, action: string, details: Record<string,unknown> = {}) => {
    const revision=task.latest_revision;
    const concurrency=revision&&!['submit','reopen','assign_annotator'].includes(action)?{revision_number:revision.number,expected_revision_version:revision.version}:{};
    const updated = await post<ReviewTask>(`${base}/${task.id}/actions`,{ expected_version:task.version,...concurrency,...details,action });
    setTasks((current) => current.map((t) => t.id === updated.id ? updated : t));return updated;
  };
  const create = async (request: unknown) => { const task = await post<ReviewTask>(base,request);setTasks((current) => [task,...current]);return task; };
  const current = async (id: string) => { const values = await api<ReviewTask[]>(base);setTasks(values);const task = values.find((t) => t.id === id);if(!task) throw new Error("Task no longer exists. Refresh the project.");return task; };
  return { tasks,loading,refresh,action,create,current };
}
export type ReviewController = ReturnType<typeof useReviewTasks>;
