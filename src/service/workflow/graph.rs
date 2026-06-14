use std::collections::HashMap;

/// True when every task is in a terminal state.
pub fn all_terminal(states: &HashMap<String, String>) -> bool {
    states.values().all(|s| {
        matches!(
            s.as_str(),
            "success" | "failed" | "upstream_failed" | "skipped"
        )
    })
}
