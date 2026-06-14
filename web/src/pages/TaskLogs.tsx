import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { api, TaskInstance, TaskLog } from '../api'

export default function TaskLogs() {
  const { taskInstanceId } = useParams<{ taskInstanceId: string }>()
  const [task, setTask] = useState<TaskInstance | null>(null)
  const [logs, setLogs] = useState<TaskLog[]>([])
  const [err, setErr] = useState('')
  const [stream, setStream] = useState<'all' | 'stdout' | 'stderr'>('all')

  const load = () => {
    if (!taskInstanceId) return
    Promise.all([
      api.getTaskLogs(taskInstanceId),
      // Reuse task info from parent if available; otherwise do a cheap fallback
    ])
      .then(([l]) => setLogs(l))
      .catch(e => setErr(e.message))
  }

  useEffect(() => { load() }, [taskInstanceId])

  const filtered = logs.filter(l => stream === 'all' || l.stream === stream)
  const combined = filtered.map(l => l.content).join('')

  return (
    <>
      <div className="flex-row" style={{ marginBottom: '1rem' }}>
        <Link to="/">← Workflows</Link>
        <h1 style={{ margin: 0 }}>Task logs</h1>
        <span className="muted">{taskInstanceId?.slice(0, 8)}…</span>
      </div>

      {err && <p className="error-text">{err}</p>}

      <div className="flex-row mt1" style={{ marginBottom: '0.75rem' }}>
        {(['all', 'stdout', 'stderr'] as const).map(s => (
          <button
            key={s}
            className={stream === s ? 'primary' : ''}
            onClick={() => setStream(s)}
          >
            {s}
          </button>
        ))}
      </div>

      {combined ? (
        <pre className="log-box">{combined}</pre>
      ) : (
        <p className="muted">No logs captured for this task yet.</p>
      )}
    </>
  )
}
