export interface Account {
  id: string;
  username: string;
  name: string;
  permissions: string[];
  group_names?: string[];
}
export interface ManagedUser extends Account { enabled: boolean; groups: string[] }
export interface Group { id: string; name: string; permissions: string[] }
export interface Member { effective_permissions?: string[]; user_id: string; name?: string; enabled?: boolean; groups: string[]; permissions: string[]; denied_permissions: string[] }
export interface WorkflowSettings { independent_reviewer: boolean; require_approval_for_datasets: boolean; session_hours: number }
export interface Session { setup_required: boolean; user: Account | null; permission_catalog: string[]; workflow: WorkflowSettings }
