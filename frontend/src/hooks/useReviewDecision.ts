import { useState } from "react";
import type { ReviewController } from "./useReviewTasks";
import type { ReviewRevision, ReviewTask } from "../types/reviews";

export function useReviewDecision(controller: ReviewController, task: ReviewTask, revision: ReviewRevision, onError: (message: string) => void) {
  const [busy, setBusy] = useState(false);
  const act = async (action: string, details: Record<string, unknown> = {}) => {
    setBusy(true);
    try {
      let current: ReviewTask = { ...task, latest_revision: revision };
      // A decision can be made immediately. The server still records a valid
      // claim/start transition and enforces reviewer assignment and permissions.
      if (action !== "claim" && !revision.review_started_at) current = await controller.action(current, "claim");
      await controller.action(current, action, details);
      return true;
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      await controller.refresh().catch(() => {});
      return false;
    } finally { setBusy(false); }
  };
  return { busy, act };
}
