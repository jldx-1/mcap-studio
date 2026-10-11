import type { Account } from "../types/accounts";
import type { Project } from "../types/models";
export const can = (project: Project | null, user: Account | null, permission: string) =>
  (project?.permissions || user?.permissions || []).includes(permission);
