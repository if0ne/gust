import { load as parseYaml } from 'js-yaml'

const BASE = '/api'

async function req<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(BASE + path, opts)
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error ?? res.statusText)
  }
  return res.json()
}

export interface WorkflowSummary {
  workflow_id: string
  schedule: string
  catchup: boolean
  is_active: boolean
  last_run_state: string | null
  last_run_at: string | null
}

export interface WorkflowDetail {
  workflow_id: string
  yaml_source: string
  spec: unknown
  schedule: string
  catchup: boolean
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface WorkflowRun {
  id: string
  workflow_id: string
  logical_date: string
  state: string
  run_type: string
  started_at: string | null
  finished_at: string | null
  created_at: string
}

export interface TaskInstance {
  id: string
  workflow_run_id: string
  task_id: string
  state: string
  try_number: number
  max_retries: number
  started_at: string | null
  finished_at: string | null
  exit_code: number | null
  error: string | null
  created_at: string
}

export interface TaskLog {
  id: number
  task_instance_id: string
  try_number: number
  stream: string
  content: string
  created_at: string
}

export const api = {
  listWorkflows: () => req<WorkflowSummary[]>('/workflows'),
  getWorkflow: (id: string) => req<WorkflowDetail>(`/workflows/${id}`),
  // The API accepts JSON; the user authors YAML, so we convert here.
  // `parseYaml` throws a YAMLException (with a useful message) on malformed input.
  createWorkflow: (yamlText: string) => {
    const spec = parseYaml(yamlText)
    return req<{ workflow_id: string }>('/workflows', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(spec),
    })
  },
  pauseWorkflow: (id: string) => req<unknown>(`/workflows/${id}/pause`, { method: 'POST' }),
  unpauseWorkflow: (id: string) => req<unknown>(`/workflows/${id}/unpause`, { method: 'POST' }),
  triggerWorkflow: (id: string) => req<{ run_id: string }>(`/workflows/${id}/trigger`, { method: 'POST' }),
  listRuns: (workflowId: string) => req<WorkflowRun[]>(`/workflows/${workflowId}/runs`),
  getRun: (runId: string) => req<WorkflowRun>(`/runs/${runId}`),
  listRunTasks: (runId: string) => req<TaskInstance[]>(`/runs/${runId}/tasks`),
  getTaskLogs: (taskInstanceId: string) => req<TaskLog[]>(`/tasks/${taskInstanceId}/logs`),
}
