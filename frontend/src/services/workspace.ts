import type { Project, ProjectSummary, Recording, RecordingSummary, Workflow } from "../types/models";
import { recordingPath } from "../utils/recordings";
import { api, post, put } from "./http";

/** Data access for project navigation, imports and the processing workspace. */
export const workspaceApi = {
  health: () => api<{ status: string }>("/health"),
  listProjects: () => api<ProjectSummary[]>("/projects"),
  project: (id: string) => api<Project>(`/projects/${id}`),
  openProject: (path: string) => post<Project>("/projects/open", { path }),
  createProject: (name: string) => post<Project>("/projects", { name }),
  recording: (project: string, recording: string) => api<Recording>(recordingPath(project, recording)),
  importRecording: (project: string, file: File) => {
    const form = new FormData();
    form.append("file", file);
    return api<RecordingSummary>(`/projects/${project}/recordings?defer_index=true`, { method: "POST", body: form });
  },
  deleteRecording: (project: string, recording: string) => api<Project>(recordingPath(project, recording), { method: "DELETE" }),
  saveWorkflow: (project: string, recording: string, workflow: Workflow) => put(`${recordingPath(project, recording)}/workflow`, workflow),
  runPipeline: (project: string, recording: string, workflow: Workflow) => post<Recording>(`/projects/${project}/pipeline`, { recording_uuid: recording, ...workflow }),
};
