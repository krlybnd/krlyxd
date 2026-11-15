/// Domain-neutral admission and retention bounds; byte scopes belong to each runner.
#[derive(Clone, Copy, Debug)]
pub struct RunnerLimits {
    pub max_live_workers: usize,
    pub max_retained_operations: usize,
    pub max_request_id_bytes: usize,
    pub max_operation_id_bytes: usize,
    pub max_input_bytes: usize,
    pub max_queued_events: usize,
    pub max_queued_bytes: usize,
}

impl Default for RunnerLimits {
    /// At most 64 retained queues of 16 MiB (1 GiB total canonical queued bytes).
    /// This is an operational default, not a bound on solver or process memory.
    fn default() -> Self {
        Self {
            max_live_workers: 4,
            max_retained_operations: 64,
            max_request_id_bytes: 256,
            max_operation_id_bytes: 64,
            max_input_bytes: 4096,
            max_queued_events: 2,
            max_queued_bytes: 16 * 1024 * 1024,
        }
    }
}

impl RunnerLimits {
    /// Valid bounds reserve a Started event and one terminal event.
    pub fn is_valid(self) -> bool {
        self.max_live_workers > 0
            && self.max_retained_operations > 0
            && self.max_request_id_bytes > 0
            && self.max_operation_id_bytes > 0
            && self.max_input_bytes > 0
            && self.max_queued_events >= 2
            && self.max_queued_bytes > 0
    }
}
