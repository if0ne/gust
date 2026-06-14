use std::{collections::HashMap, num::NonZeroU32};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub component: super::component::ComponentSpec,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowSpec {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_active_tasks: Option<NonZeroU32>,
    pub tasks: Vec<TaskSpec>,
}

impl WorkflowSpec {
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        let mut graph = petgraph::graph::DiGraph::new();
        let mut indices = HashMap::new();

        for task in &self.tasks {
            if task.id.is_empty() {
                return Err(anyhow::anyhow!("task id must not be empty"));
            }

            if indices.contains_key(&task.id) {
                return Err(anyhow::anyhow!("duplicate task id: {}", task.id));
            }

            let idx = graph.add_node(task.id.clone());
            indices.insert(task.id.clone(), idx);
        }

        for task in &self.tasks {
            let &to = indices.get(&task.id).unwrap();
            for dep in &task.depends_on {
                let &from = indices.get(dep).ok_or_else(|| {
                    anyhow::anyhow!("task '{}' depends_on unknown task '{}'", task.id, dep)
                })?;
                graph.add_edge(from, to, ());
            }
        }

        petgraph::algo::toposort(&graph, None).map_err(|c| {
            anyhow::anyhow!(
                "cycle detected in workflow '{}' at task '{}'",
                self.id,
                graph[c.node_id()]
            )
        })?;

        Ok(())
    }

    /// Returns task IDs that should move from pending → queued.
    pub fn ready_tasks<'a>(
        &'a self,
        states: &HashMap<String, String>,
    ) -> impl Iterator<Item = &'a str> {
        self.tasks
            .iter()
            .filter(|t| {
                states.get(&t.id).map(|s| s == "pending").unwrap_or(false)
                    && t.depends_on
                        .iter()
                        .all(|dep| states.get(dep).map(|s| s == "success").unwrap_or(false))
            })
            .map(|t| t.id.as_str())
    }

    /// Returns task IDs that should move to upstream_failed.
    pub fn upstream_failed_tasks<'a>(
        &'a self,
        states: &HashMap<String, String>,
    ) -> impl Iterator<Item = &'a str> {
        self.tasks
            .iter()
            .filter(|t| {
                states.get(&t.id).map(|s| s == "pending").unwrap_or(false)
                    && t.depends_on.iter().any(|dep| {
                        states
                            .get(dep)
                            .map(|s| s == "failed" || s == "upstream_failed")
                            .unwrap_or(false)
                    })
            })
            .map(|t| t.id.as_str())
    }
}
