import { useEffect, useState } from "react";
import { api } from "../services/http";
import type { Recording } from "../types/models";
import type { ReviewRevision, ReviewTask } from "../types/reviews";

export function useReviewRevision(projectId: string, task: ReviewTask | undefined, onError: (message: string) => void) {
  const [number, setNumber] = useState(0);
  const [history, setHistory] = useState<{taskId:string;values:ReviewRevision[]}|null>(null);
  const [snapshot, setSnapshot] = useState<{taskId:string;number:number;recording:Recording}|null>(null);
  const revisions = history?.taskId === task?.id ? history?.values.map((value) => value.number === task?.latest_revision?.number ? task.latest_revision : value) || [] : [];
  const revision = task && number === task.current_revision ? task.latest_revision || revisions.find((value) => value.number === number) : revisions.find((value) => value.number === number);
  const recording = snapshot && task && snapshot.taskId === task.id && snapshot.number === number ? snapshot.recording : null;

  useEffect(() => {
    if (!task) return;
    let active = true;
    void api<ReviewRevision[]>(`/projects/${projectId}/tasks/${task.id}/revisions`).then((values) => {
      if (active) {
        setHistory({taskId:task.id,values});
        setNumber((current) => values.some((value) => value.number === current) ? current : task.current_revision);
      }
    }).catch((err) => { if (active) onError(String(err)); });
    return () => { active = false; };
  }, [projectId, task?.id, task?.current_revision, task?.latest_revision?.version, onError]);

  useEffect(() => {
    setSnapshot(null);
    if (!task || !number || !revision?.snapshot_available) return;
    let active = true;
    void api<Recording>(`/projects/${projectId}/tasks/${task.id}/revisions/${number}/recording`).then((value) => {
      if (active) setSnapshot({taskId:task.id,number,recording:value});
    }).catch((err) => { if (active) onError(String(err)); });
    return () => { active = false; };
  }, [projectId, task?.id, number, revision?.snapshot_available, onError]);

  return { revisions, revision, recording, number, setNumber };
}
