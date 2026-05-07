//! Single source of truth for the warm-up model. When Anthropic ships a
//! cheaper / newer Haiku, the rename is one line here.

pub const WARMUP_MODEL: &str = "claude-haiku-4-5";

/// Hard cap on the per-call HTTP timeout. Per spec §6.
pub const WARMUP_HTTP_TIMEOUT_SECS: u64 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_model_is_haiku() {
        assert!(WARMUP_MODEL.starts_with("claude-haiku"));
    }
}
