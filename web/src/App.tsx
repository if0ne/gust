import { BrowserRouter, Link, Route, Routes } from 'react-router-dom'
import WorkflowList from './pages/WorkflowList'
import WorkflowDetail from './pages/WorkflowDetail'
import RunList from './pages/RunList'
import RunDetail from './pages/RunDetail'
import TaskLogs from './pages/TaskLogs'

export default function App() {
  return (
    <BrowserRouter>
      <nav>
        <span className="brand">⚡ Gust</span>
        <Link to="/">Workflows</Link>
      </nav>
      <main>
        <Routes>
          <Route path="/" element={<WorkflowList />} />
          <Route path="/workflows/:workflowId" element={<WorkflowDetail />} />
          <Route path="/workflows/:workflowId/runs" element={<RunList />} />
          <Route path="/runs/:runId" element={<RunDetail />} />
          <Route path="/tasks/:taskInstanceId/logs" element={<TaskLogs />} />
        </Routes>
      </main>
    </BrowserRouter>
  )
}
