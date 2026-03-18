/// Check whether there is enough remaining budget in the 5-hour usage window
/// to run another step (implement or review).
///
/// Returns `true` if we can continue, `false` if we should defer to the next tick.
///
/// TODO: Implement actual 5h usage window check. Possible approaches:
/// - Parse `claude` CLI usage output
/// - Query Anthropic API usage endpoint
/// - Heuristic based on recent activity durations from activity.jsonl
pub fn has_budget() -> bool {
    // For now, always continue — the full flow runs all steps in one tick.
    true
}
