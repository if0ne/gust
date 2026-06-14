import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { api, WorkflowDetail as WorkflowDetailType } from '../api'

interface TaskSpec {
  id: string
  depends_on?: string[]
}

interface Spec {
  tasks?: TaskSpec[]
}

export default function WorkflowDetail() {
  const { workflowId } = useParams<{ workflowId: string }>()
  const [workflow, setWorkflow] = useState<WorkflowDetailType | null>(null)
  const [err, setErr] = useState('')

  useEffect(() => {
    if (workflowId) api.getWorkflow(workflowId).then(setWorkflow).catch(e => setErr(e.message))
  }, [workflowId])

  if (err) return <p className="error-text">{err}</p>
  if (!workflow) return <p className="muted">Loading…</p>

  const spec = workflow.spec as Spec
  const tasks = spec?.tasks ?? []

  return (
    <>
      <div className="flex-row" style={{ marginBottom: '1rem' }}>
        <Link to="/">← Workflows</Link>
        <span style={{ color: '#718096' }}>/</span>
        <h1 style={{ margin: 0 }}>{workflow.workflow_id}</h1>
        <span className={`badge badge-${workflow.is_active ? 'success' : 'pending'}`}>
          {workflow.is_active ? 'active' : 'paused'}
        </span>
      </div>

      <div className="card">
        <div className="flex-row">
          <span className="muted">Schedule:</span>
          <code style={{ fontSize: '0.85rem' }}>{workflow.schedule}</code>
          <span className="muted">Catchup: {workflow.catchup ? 'yes' : 'no'}</span>
        </div>
      </div>

      <div className="card">
        <h2>Task graph</h2>
        <div className="graph">
          {tasks.map(t => (
            <div key={t.id} className="graph-node" title={`depends_on: ${(t.depends_on ?? []).join(', ') || 'none'}`}>
              {t.id}
              {(t.depends_on ?? []).length > 0 && (
                <span className="muted" style={{ marginLeft: '0.4rem', fontSize: '0.75rem' }}>
                  ← {t.depends_on!.join(', ')}
                </span>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="card">
        <h2>YAML source</h2>
        <pre className="log-box">{workflow.yaml_source}</pre>
      </div>

      <div className="mt2">
        <Link to={`/workflows/${workflow.workflow_id}/runs`}><button>View runs →</button></Link>
      </div>
    </>
  )
}
