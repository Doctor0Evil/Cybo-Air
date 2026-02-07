use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{BeeKarmaEnvelope, BeeKarma};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeeTwinSnapshot {
    pub twin_id: Uuid,
    pub corridor_id: Uuid,
    pub t: DateTime<Utc>,
    pub vg_pred: f64,
    pub vg_obs: f64,
    pub dwv_pred: f64,
    pub dwv_obs: f64,
    pub weight_pred: f64,
    pub weight_obs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmAggregation {
    pub corridor_id: Uuid,
    pub predicted_harm: f64,
    pub realized_harm: f64,
    pub delta_liability: f64,
}

pub fn aggregate_harm(
    snapshots: &[BeeTwinSnapshot],
    w_v: f64,
    w_d: f64,
    w_w: f64,
) -> HarmAggregation {
    assert!(!snapshots.is_empty());

    let corridor_id = snapshots[0].corridor_id;
    let mut realized_sum = 0.0;
    let mut predicted_sum = 0.0;

    for s in snapshots {
        let h_real = w_v * (s.vg_pred - s.vg_obs).abs()
            + w_d * (s.dwv_pred - s.dwv_obs).abs()
            + w_w * (s.weight_pred - s.weight_obs).abs();

        // For now, predicted harm is simply the model's own expectation of zero residual.
        // In a full implementation, predicted_harm would be drawn from ApisRAM/BEEHAVE runs.
        let h_pred = 0.0_f64;

        realized_sum += h_real;
        predicted_sum += h_pred;
    }

    let n = snapshots.len() as f64;
    HarmAggregation {
        corridor_id,
        predicted_harm: predicted_sum / n,
        realized_harm: realized_sum / n,
        delta_liability: realized_sum / n - predicted_sum / n,
    }
}

pub fn apply_liability_to_envelope(
    env: &mut BeeKarmaEnvelope,
    harm: &HarmAggregation,
    warn_threshold: f64,
    downgrade_threshold: f64,
    karma_penalty_scale: f64,
) {
    env.predicted_harm_score = harm.predicted_harm;
    env.realized_harm_score = harm.realized_harm;

    if harm.delta_liability <= warn_threshold {
        return;
    }

    let delta_over = harm.delta_liability - warn_threshold;
    let karma_delta = -karma_penalty_scale * delta_over;
    let mut k = env.kappa.0 + karma_delta;
    if k < 0.0 {
        k = 0.0;
    }
    env.kappa = BeeKarma(k);

    env.blood_gate_level = if k >= 0.8 {
        3
    } else if k >= 0.6 {
        2
    } else if k >= 0.4 {
        1
    } else {
        0
    };

    // Liability trigger: if harm is very high, force immediate downgrade.
    if harm.delta_liability >= downgrade_threshold {
        env.blood_gate_level = env.blood_gate_level.saturating_sub(1);
    }
}
