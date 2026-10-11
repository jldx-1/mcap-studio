export interface ReviewScope { channel_id: string; start_frame: number; end_frame: number }
export interface ReviewNavigationTarget { taskId: string; view: "review" | "comments" | "history"; scope?: ReviewScope }
export type ReviewStatus = "unlabeled" | "in_progress" | "submitted" | "under_review" | "changes_requested" | "approved" | "superseded" | "withdrawn";
export interface ReviewEvent { at: string; actor_id: string; actor_name: string; action: string; details: { message?: string; scope?: ReviewScope; changed_frames?: number } }
export interface ReviewTask {
  id: string; recording_id: string; dataset_id?: string | null; title: string; scopes: ReviewScope[];
  annotator_id: string; annotator_name: string; reviewer_id: string | null; reviewer_name: string | null;
  status: ReviewStatus; frame_count: number; annotated_count: number; approved_sections: ReviewScope[];
  submitted_digest: string | null;
  current_revision: number;
  working_dirty: boolean;
  latest_revision?: ReviewRevision | null;
  previous_revision?: { number: number; status: ReviewStatus; comment_count: number } | null;
  version: number; history: ReviewEvent[]; created_at: string; updated_at: string;
}
export interface RevisionComment {id:string;actor_id:string;actor_name:string;at:string;message:string;scope:ReviewScope|null}
export interface ReviewRevision {id:string;task_id:string;recording_id:string;number:number;scopes:ReviewScope[];status:ReviewStatus;submitted_by_id:string;submitted_by_name:string;submitted_at:string;reviewed_at:string|null;review_started_at:string|null;self_review?:boolean;reviewer_id:string|null;reviewer_name:string|null;comments:RevisionComment[];version:number;frame_count:number;annotated_count:number;snapshot_available:boolean;snapshot:{path:string;sha256:string}}
export interface RevisionDiff {base:number;revision:number;total:number;changes:{channel_id:string;frame:number;kind:"added"|"deleted"|"modified";before:unknown;after:unknown}[];labels_changed:boolean;ranges_changed:boolean;csv_changed:boolean}
export const statusLabel: Record<ReviewStatus, string> = {
  unlabeled: "Not started", in_progress: "Annotating", submitted: "Ready for review",
  under_review: "Under review", changes_requested: "Changes requested", approved: "Approved",
  superseded:"Superseded",withdrawn:"Withdrawn",
};
