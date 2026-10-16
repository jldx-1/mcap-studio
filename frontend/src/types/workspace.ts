

export type Page =

  | "projects"
  | "project"
  | "mcaps"
  | "labels"
  | "topics"
  | "structured"
  | "labeling"
  | "pipeline"
  | "prepare"
  | "datasets"
  | "administration"
  | "account"
  | "permissions"
  | "members"
  | "reviews"
  | "review"
  ;

export type Theme = "light" | "dark" | "system";

export type Perform = (
  label: string,
  action: () => Promise<void>,
) => Promise<boolean>;
