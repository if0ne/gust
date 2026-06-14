import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { api, WorkflowRun } from '../api'

export default function RunList() {
  const { workflowId } = useParams<{ workflowId: string }>()
  const [runs, setRuns] = useState<WorkflowRun[]>([])
  const [err, setErr] = useState('')

  useEffect(() => {
    if (workflowId) api.listRuns(workflowId).then(setRuns).catch(e => setErr(e.message))
  }, [workflowId])

  return (
    <>
      <div className="flex-row" style={{ marginBottom: '1rem' }}>
        <Link to="/">← Workflows</Link>
        <span style={{ color: '#718096' }}>/</span>
        <Link to={`/workflows/${workflowId}`}>{workflowId}</Link>
        <span style={{ color: '#718096' }}>/</span>
        <h1 style={{ margin: 0 }}>Runs</h1>
      </div>

      {err && <p className="error-text">{err}</p>}

      <table>
        <thead>
          <tr>
            <th>Run ID</th>
            <th>Logical date</th>
            <th>Type</th>
            <th>State</th>
            <th>Started</th>
            <th>Finished</th>
          </tr>
        </thead>
        <tbody>
          {runs.map(r => (
            <tr key={r.id}>
              <td><Link to={`/runs/${r.id}`}>{r.id.slice(0, 8)}…</Link></td>
              <td>{new Date(r.logical_date).toLocaleString()}</td>
              <td>{r.run_type}</td>
              <td><span className={`badge badge-${r.state}`}>{r.state}</span></td>
              <td className="muted">{r.started_at ? new Date(r.started_at).toLocaleString() : '—'}</td>
              <td className="muted">{r.finished_at ? new Date(r.finished_at).toLocaleString() : '—'}</td>
            </tr>
          ))}
          {runs.length === 0 && (
            <tr><td colSpan={6} className="muted" style={{ textAlign: 'center', padding: '2rem' }}>No runs yet.</td></tr>
          )}
        </tbody>
      </table>
    </>
  )
}
