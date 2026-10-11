

export interface ProjectSummary {
  path?: string;
  uuid: string;
  name: string;
  recording_count: number;
  label_count: number;
  dataset_count: number;
}

export interface RecordingMetrics {
  topic_count: number;
  topic_names: string[];
  duration_seconds: number;
  modalities: string[];
  frame_count: number;
  annotated_count: number;
  reviewed_count: number;
}

export interface RecordingSummary {
  created_at?: string;
  summary?: RecordingMetrics;
  uuid: string;
  source_file: string;
  sha256: string;
}

export interface Label {
  uuid: string;
  name: string;
}

export interface LabelList {
  uuid: string;
  name: string;
  label_ids: string[];
}

export interface DatasetSource {
  mcap_uuid: string;
  channel_id: string;
  source_file: string;
  topic: string;
  modality: string;
  entry_count: number;
}

export interface Dataset {
  kind?: string;
  latest_version?: number;
  versions?: import("./datasetVersions").VersionSummary[];
  sources?: DatasetSource[];
  modalities?: string[];
  multimodal?: boolean;
  source_recording_uuid?: string;
  source_channel?: string;
  overrides?: Record<string, Record<string, string>>;
  format?: string;
  csv_path?: string;
  columns?: { name: string }[];
  uuid: string;
  name: string;
  entry_count: number;
  entries?: Record<string, Record<string, unknown>> | Record<string, unknown>[];
}

export interface Project extends ProjectSummary {
  permissions?: string[];
  path?: string;
  mcaps: RecordingSummary[];
  labels: Label[];
  label_lists: LabelList[];
  datasets: Dataset[];
}

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
  class_id: number;
  track_id?: number;
  origin?: string;
}

export interface FrameAnnotation {
  reviewed: boolean;
  boxes: Box[];
  labels?: number[];
  excluded_tracks?: number[];
  fields?: unknown;
  [key: string]: unknown;
}

export interface AnnotationState {
  format_version: number;
  sha256: string;
  classes: string[];
  class_uuids?: (string | null)[];
  techniques: Record<string, string>;
  annotations: Record<string, Record<string, FrameAnnotation>>;
  settings: {
    step?: number;
    positions?: Record<string, number>;
    [key: string]: unknown;
  };
  topic_labels?: Record<string, string[]>;
  topic_label_lists?: Record<string, string>;
  [key: string]: unknown;
}

export interface Frame {
  time_ns: number | string;
  image?: string;
  width?: number;
  height?: number;
  offset?: number;
}

export interface Topic {
  channel_id: string | number;
  topic: string;
  kind: string;
  encoding: string;
  schema_name?: string;
  frame_count: number;
  technique?: string;
  errors?: string[];
}

export interface Workflow {
  selected_channels: string[];
  translations: Record<string, string>;
  sync: {
    enabled: boolean;
    parent_channel: string;
    mode: string;
    tolerance_ns: number | null;
    delete_out_of_sync: boolean;
  };
}

export interface Recording extends RecordingSummary {
  eligible_counts?: Record<string, number>;
  revision_datasets?: {uuid:string;name:string;source_channel:string}[];
  topics: Topic[];
  index: {
    topics: Record<string, { frames: Frame[];[key: string]: unknown }>;
  };
  annotations: AnnotationState;
  workflow?: Workflow;
}

export interface ExportResult {
  uuid: string;
  path: string;
  files: string[];
  download_url?: string;
}

export interface SavedAnnotationPatch {
  sha256: string;
  metadata: Partial<AnnotationState>;
  changes: { channel_id: string; frame: number; annotation: FrameAnnotation | null }[];
}
