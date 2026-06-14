import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { api, WorkflowRun, TaskInstance } from '../api'

const STATE_COLORS: Record<string, string> = {
  success: '#22543d',
  failed: '#63171b',
  running: '#2a4365',
  queued: '#3d2c00',
  pending: '#2d3748',
  upstream_failed: '#44337a',
  skipped: '#2d3748',
}

export default function RunDetail() {
  const { runId } = useParams<{ runId: string }>()
  const [run, setRun] = useState<WorkflowRun | null>(null)
  const [tasks, setTasks] = useState<TaskInstance[]>([])
  const [err, setErr] = useState('')

  const load = () => {
    if (!runId) return
    Promise.all([api.getRun(runId), api.listRunTasks(runId)])
      .then(([r, t]) => { setRun(r); setTasks(t) })
      .catch(e => setErr(e.message))
  }

  useEffect(() => {
    load()
    const timer = setInterval(load, 3000)
    return () => clearInterval(timer)
  }, [runId])

  if (err) return <p className="error-text">{err}</p>
  if (!run) return <p className="muted">Loading…</p>

  return (
    <>
      <div className="flex-row" style={{ marginBottom: '1rem' }}>
        <Link to="/">← Workflows</Link>
        <span style={{ color: '#718096' }}>/</span>
        <Link to={`/workflows/${run.workflow_id}/runs`}>{run.workflow_id}</Link>
        <span style={{ color: '#718096' }}>/</span>
        <h1 style={{ margin: 0 }}>Run {run.id.slice(0, 8)}…</h1>
        <span className={`badge badge-${run.state}`}>{run.state}</span>
      </div>

      <div className="card">
        <div className="flex-row">
          <span className="muted">Logical date:</span>
          <span>{new Date(run.logical_date).toLocaleString()}</span>
          <span className="muted">Type: {run.run_type}</span>
          {run.started_at && <span className="muted">Started: {new Date(run.started_at).toLocaleString()}</span>}
          {run.finished_at && <span className="muted">Finished: {new Date(run.finished_at).toLocaleString()}</span>}
        </div>
      </div>

      <div className="card">
        <h2>Task graph</h2>
        <div className="graph">
          {tasks.map(t => (
            <Link key={t.id} to={`/tasks/${t.id}/logs`} style={{ textDecoration: 'none' }}>
              <div
                className="graph-node"
                style={{ background: STATE_COLORS[t.state] ?? '#2d3748', borderColor: STATE_COLORS[t.state] ?? '#4a5568' }}
              >
                {t.task_id}
                <span className="muted" style={{ marginLeft: '0.4rem', fontSize: '0.75rem' }}>{t.state}</span>
                {t.try_number > 0 && <span className="muted" style={{ marginLeft: '0.3rem', fontSize: '0.72rem' }}>#{t.try_number}</span>}
              </div>
            </Link>
          ))}
        </div>
      </div>

      <table>
        <thead>
          <tr>
            <th>Task</th>
            <th>State</th>
            <th>Try</th>
            <th>Exit code</th>
            <th>Error</th>
            <th>Started</th>
            <th>Finished</th>
            <th>Logs</th>
          </tr>
        </thead>
        <tbody>
          {tasks.map(t => (
            <tr key={t.id}>
              <td>{t.task_id}</td>
              <td><span className={`badge badge-${t.state}`}>{t.state}</span></td>
              <td>{t.try_number}/{t.max_retries}</td>
              <td className="muted">{t.exit_code ?? '—'}</td>
              <td className="muted" style={{ maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis' }}>{t.error ?? '—'}</td>
              <td className="muted">{t.started_at ? new Date(t.started_at).toLocaleString() : '—'}</td>
              <td className="muted">{t.finished_at ? new Date(t.finished_at).toLocaleString() : '—'}</td>
              <td><Link to={`/tasks/${t.id}/logs`}><button>Logs</button></Link></td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  )
}
