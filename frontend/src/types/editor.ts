import type { AnnotationState, Project, Recording } from "./models";

export type AnnotationEditorProps = {
  readOnly?: boolean;
  project: Project;
  recording: Recording;
  channel: string;
  onBack: () => void;
  onLabels: () => void;
  onError: (value: string) => void;
  onNotice: (value: string) => void;
  onSaved: (value: AnnotationState) => void;
  registerNavigationGuard: (guard: (() => Promise<boolean>) | null) => void;
};
