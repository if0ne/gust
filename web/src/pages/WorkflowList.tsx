import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { api, WorkflowSummary } from '../api'

function Badge({ state }: { state: string | null }) {
  if (!state) return <span className="muted">—</span>
  return <span className={`badge badge-${state}`}>{state}</span>
}

export default function WorkflowList() {
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([])
  const [err, setErr] = useState('')
  const [yaml, setYaml] = useState('')
  const [uploading, setUploading] = useState(false)
  const nav = useNavigate()

  const load = () => api.listWorkflows().then(setWorkflows).catch(e => setErr(e.message))

  useEffect(() => { load() }, [])

  const pause = async (id: string, active: boolean) => {
    try {
      active ? await api.pauseWorkflow(id) : await api.unpauseWorkflow(id)
      load()
    } catch (e: any) { setErr(e.message) }
  }

  const trigger = async (id: string) => {
    try {
      const { run_id } = await api.triggerWorkflow(id)
      nav(`/runs/${run_id}`)
    } catch (e: any) { setErr(e.message) }
  }

  const upload = async () => {
    if (!yaml.trim()) return
    setUploading(true)
    try {
      await api.createWorkflow(yaml)
      setYaml('')
      load()
    } catch (e: any) { setErr(e.message) }
    finally { setUploading(false) }
  }

  return (
    <>
      <h1>Workflows</h1>
      {err && <p className="error-text mt1">{err}</p>}

      <div className="card mt2">
        <h2>Upload Workflow YAML</h2>
        <textarea
          style={{ width: '100%', height: 140, background: '#0a0c14', color: '#a0aec0', border: '1px solid #4a5568', borderRadius: 4, padding: '0.5rem', fontFamily: 'monospace', fontSize: '0.82rem' }}
          value={yaml}
          onChange={e => setYaml(e.target.value)}
          placeholder="Paste YAML here…"
        />
        <div className="mt1">
          <button className="primary" onClick={upload} disabled={uploading}>
            {uploading ? 'Uploading…' : 'Upload'}
          </button>
        </div>
      </div>

      <table>
        <thead>
          <tr>
            <th>Workflow</th>
            <th>Schedule</th>
            <th>Active</th>
            <th>Last run</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {workflows.map(d => (
            <tr key={d.workflow_id}>
              <td><Link to={`/workflows/${d.workflow_id}`}>{d.workflow_id}</Link></td>
              <td><code style={{ fontSize: '0.8rem' }}>{d.schedule}</code></td>
              <td>{d.is_active ? '✅' : '⏸'}</td>
              <td>
                <Badge state={d.last_run_state} />
                {d.last_run_at && (
                  <span className="muted" style={{ marginLeft: '0.4rem' }}>
                    {new Date(d.last_run_at).toLocaleString()}
                  </span>
                )}
              </td>
              <td>
                <div className="flex-row">
                  <button onClick={() => trigger(d.workflow_id)}>▶ Trigger</button>
                  <button onClick={() => pause(d.workflow_id, d.is_active)}>
                    {d.is_active ? '⏸ Pause' : '▶ Unpause'}
                  </button>
                  <Link to={`/workflows/${d.workflow_id}/runs`}><button>Runs</button></Link>
                </div>
              </td>
            </tr>
          ))}
          {workflows.length === 0 && (
            <tr><td colSpan={5} className="muted" style={{ textAlign: 'center', padding: '2rem' }}>No workflows yet. Upload a YAML above.</td></tr>
          )}
        </tbody>
      </table>
    </>
  )
}
