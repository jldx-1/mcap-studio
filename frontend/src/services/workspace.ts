import type { Project, ProjectSummary, Recording, RecordingSummary, Workflow } from "../types/models";
import { recordingPath } from "../utils/recordings";
import { api, post, put } from "./http";

export interface ImportedRecording extends RecordingSummary {
  duplicate?: boolean;
  source_recording?: RecordingSummary;
  translated_topics?: number;
  translation_warning?: string;
}

function uploadSize(bytes: number): string {
  const unit = bytes >= 1024 ** 3 ? "GiB" : bytes >= 1024 ** 2 ? "MiB" : bytes >= 1024 ? "KiB" : "bytes";
  const divisor = unit === "GiB" ? 1024 ** 3 : unit === "MiB" ? 1024 ** 2 : unit === "KiB" ? 1024 : 1;
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 }).format(bytes / divisor)} ${unit}`;
}

/** Data access for project navigation, imports and the processing workspace. */
export const workspaceApi = {
  health: () => api<{ status: string }>("/health"),
  listProjects: () => api<ProjectSummary[]>("/projects"),
  project: (id: string) => api<Project>(`/projects/${id}`),
  openProject: (path: string) => post<Project>("/projects/open", { path }),
  createProject: (name: string) => post<Project>("/projects", { name }),
  recording: (project: string, recording: string) => api<Recording>(recordingPath(project, recording)),
  importRecording: async (project: string, file: File, autoTranslate = false) => {
    const { max_mcap_bytes: limit } = await api<{ max_mcap_bytes: number | null }>(`/projects/${project}/upload-limits`);
    if (limit !== null && file.size > limit) {
      throw new Error(`File is too large (${uploadSize(file.size)}). The maximum MCAP upload size is ${uploadSize(limit)}. Choose a smaller file.`);
    }
    const form = new FormData();
    form.append("file", file);
    return api<ImportedRecording>(`/projects/${project}/recordings?defer_index=true&auto_translate=${autoTranslate}`, { method: "POST", body: form });
  },
  deleteRecording: (project: string, recording: string) => api<Project>(recordingPath(project, recording), { method: "DELETE" }),
  saveWorkflow: (project: string, recording: string, workflow: Workflow) => put(`${recordingPath(project, recording)}/workflow`, workflow),
  runPipeline: (project: string, recording: string, workflow: Workflow) => post<Recording>(`/projects/${project}/pipeline`, { recording_uuid: recording, ...workflow }),
  applyTranslations: (project: string, recording: string, selections: Record<string,string>) => post<Recording>(`${recordingPath(project,recording)}/translations/apply`, {selections}),
};
