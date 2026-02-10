#![forbid(unsafe_code)]

use serde::Deserialize;

/// Minimal control proposal schema seen at the governance boundary.
/// The LLM or UI may only send this shape, never arbitrary commands.
#[derive(Debug, Clone, Deserialize)]
pub struct ControlProposal {
    pub node_id: String,
    pub new_duty_cycle: f64,
    pub horizon_seconds: u64,
}

/// InputGuard: first line of defense against malformed or hostile payloads.
pub struct InputGuard;

impl InputGuard {
    pub fn validate_control_proposal(p: &ControlProposal) -> Result<(), String> {
        if p.node_id.is_empty() {
            return Err("node_id must not be empty".into());
        }
        if !(0.0..=1.0).contains(&p.new_duty_cycle) {
            return Err("new_duty_cycle must be between 0.0 and 1.0".into());
        }
        if p.horizon_seconds == 0 {
            return Err("horizon_seconds must be > 0".into());
        }
        Ok(())
    }

    // Similar guards can be defined for:
    // - shard write payloads (qpudatashard updates),
    // - telemetry streams,
    // - export filters.
}
