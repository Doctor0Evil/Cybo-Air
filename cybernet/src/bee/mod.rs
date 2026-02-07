use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identity-bound scalar for neurorights-style integrity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BeeKarma(pub f64); // 0.0 – 1.0, hard lower bounds enforced via predicates.

/// Compact stressor vector projected into the bee-rights polytope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeeStressorState {
    pub hq_pest: f64,       // pesticide HQ
    pub h_rf: f64,          // RF-EMF hazard index
    pub h_poll: f64,        // air pollutant hazard index
    pub d_h_bio: f64,       // biomarker harm delta
    pub varroa_per_100: f64,
    pub d_thive_c: f64,     // |T_hive - 34.5|
    pub q_forage: f64,      // normalized foraging success
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeeCorridorPolytope {
    pub a: Vec<Vec<f64>>,   // rows of A
    pub b: Vec<f64>,        // bounds
    pub kappa_min: f64,     // admissible minimum bee_karma
}

/// Identity-bound governance envelope for a Cybernet agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeeKarmaEnvelope {
    pub agent_id: Uuid,
    pub corridor_id: Uuid,
    pub kappa: BeeKarma,
    pub last_update: DateTime<Utc>,
    pub realized_harm_score: f64,  // reconciled ABM vs telemetry
    pub predicted_harm_score: f64, // from bee twin simulations
    pub blood_gate_level: u8,      // 0 = revoked, 1 = read-only, 2 = limited-write, 3 = full
}
