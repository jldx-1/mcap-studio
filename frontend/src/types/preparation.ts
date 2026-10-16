import type { RecordingSummary, Workflow } from "./models";
import type { DatasetVersion } from "./datasetVersions";

export interface TranslationProfile {
  id?: string;
  name: string;
  script: string;
  topic?: string;
  source_message?: string;
  source_schema_sha256?: string;
  output_schema?: unknown;
}
export interface PreparationTopic {
  channel_id: string;
  topic: string;
  source_message: string;
  source_schema_sha256: string;
  encoding: string;
  choices: TranslationProfile[];
}
export interface TopicRule {
  topic: string;
  source_message: string;
  source_schema_sha256: string;
  keep: boolean;
  required: boolean;
  script: string;
  technique: string;
  label_list_id: string;
}
export interface PreparationRecipe {
  sha256?: string;
  uuid?: string;
  revision?: number;
  name: string;
  rules: TopicRule[];
  sync?: Workflow["sync"] & { parent_topic?: string };
}
export interface PreparationJob {
  uuid: string;
  recording_uuid: string;
  source_file: string;
  status: "queued" | "running" | "ready" | "failed";
  stage: string;
  output_recording?: string;
  error?: string;
  request: Record<string, unknown>;
}
export interface RecordingReference { project_uuid: string; recording_id: string }
export interface DatasetDraft {
  uuid?: string;
  revision?: number;
  name: string;
  recordings: RecordingReference[];
  selected_sources: string[];
  target_dataset: string | null;
  base_version?: DatasetVersion | null;
  selection?: DatasetVersion["selection"];
}
export interface LibraryRecording extends RecordingSummary {
  project_uuid: string;
  project_name: string;
  can_export: boolean;
}
