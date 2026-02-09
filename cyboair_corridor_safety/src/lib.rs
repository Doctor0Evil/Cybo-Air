use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Core row schema, aligned with Cybo-Air qpudatashards for Phoenix and similar.
/// This is intentionally close to the types you already use in cybo-air control crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorridorRow {
    pub machine_id: String,
    pub r#type: String,
    pub location: String,
    pub pollutant: String,
    pub cin: f64,
    pub cout: f64,
    pub unit: String,
    pub airflow_m3_per_s: f64,
    pub period_s: f64,
    pub lambda_hazard: f64,
    pub beta_nb_per_kg: f64,
    pub ecoimpact_score: f64,
}

/// Minimal node state needed for corridor control.
#[derive(Debug, Clone)]
pub struct NodeState {
    pub row: CorridorRow,
    pub mass_kg: f64,
    pub karma_bytes: f64,
    pub duty_cycle: f64, // u in [0,1]
    pub power_w: f64,
    pub geo_weight: f64,
}

/// Eco-band classification.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum EcoBand {
    Green,
    Amber,
    Red,
}

/// Errors for invariants and envelopes.
#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("safety envelope violated: {0}")]
    EnvelopeViolation(&'static str),
    #[error("host budget exceeded: {0}")]
    HostBudgetExceeded(&'static str),
    #[error("dw ceiling exceeded: {0}")]
    DwCeilingExceeded(&'static str),
}

/// Conversion from shard concentration units to kg/m^3.
/// This is the same operator you already use (ug/m3, mg/m3, ppb via MW,R,T).
pub fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    match unit {
        "ugm3" => 1e-9,
        "mgm3" => 1e-6,
        "ppb" => {
            let r = 8.3145_f64;
            // C(ppb) * MW / (R*T) * 1e-9 -> kg/m^3
            molar_mass_kg_per_mol / (r * temperature_k) * 1e-9
        }
        _ => 0.0,
    }
}

/// CEIM-style mass operator M = C_u * Q * t.
pub fn compute_mass_kg(row: &CorridorRow, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    let alpha = unit_to_kg_factor(&row.unit, temperature_k, molar_mass_kg_per_mol);
    let delta_c = (row.cin - row.cout).max(0.0);
    let c_u = alpha * delta_c;
    c_u * row.airflow_m3_per_s * row.period_s
}

/// Hazard-weighted NanoKarmaBytes, K = lambda * beta * M.
pub fn compute_karma_bytes(row: &CorridorRow, mass_kg: f64) -> f64 {
    row.lambda_hazard * row.beta_nb_per_kg * mass_kg
}

/// Trait for safety envelope semantics.
pub trait SafetyEnvelope {
    /// Returns Ok(()) if the node state is inside its safety envelope, Err otherwise.
    fn check_envelope(&self, node: &NodeState) -> Result<(), SafetyError>;
}

/// Trait for host-budget semantics (energy, power, liability caps).
pub trait HostBudget {
    /// Returns Ok(()) if host budgets are respected for this node in the current step.
    fn check_host_budget(&self, node: &NodeState) -> Result<(), SafetyError>;
    /// Return normalized power fraction P/P_max in [0, +inf).
    fn power_fraction(&self, node: &NodeState) -> f64;
}

/// Trait for eco-band classification at corridor scope.
pub trait EcoBandClassifier {
    /// Given corridor-wide normalized load E_corr, return eco-band.
    fn classify(&self, eco_load: f64) -> EcoBand;
    /// Optional band gain used in the duty update law.
    fn band_gain(&self, band: EcoBand) -> f64;
}

/// Trait for DW ceiling invariants over corridors.
pub trait DwCeilingInvariant {
    /// Returns Ok(()) if DW ceiling is respected, Err otherwise.
    fn check_dw_ceiling(&self, phi_dw: f64) -> Result<(), SafetyError>;
    /// Returns normalized DW violation \delta_DW (>= 0 if violated, 0 otherwise).
    fn dw_violation(&self, phi_dw: f64) -> f64;
}

/// Simple rectangular envelope over duty, altitude, and ecoimpact.
/// This is intentionally conservative; more complex A x <= b polytopes can be swapped in.
#[derive(Debug, Clone)]
pub struct RectSafetyEnvelope {
    pub u_min: f64,
    pub u_max: f64,
    pub z_min_m: f64,
    pub z_max_m: f64,
    pub ecoimpact_min: f64,
    pub ecoimpact_max: f64,
    /// Altitude map, provided externally.
    pub altitude_m: fn(&str) -> f64,
}

impl SafetyEnvelope for RectSafetyEnvelope {
    fn check_envelope(&self, node: &NodeState) -> Result<(), SafetyError> {
        let u = node.duty_cycle;
        if u < self.u_min || u > self.u_max {
            return Err(SafetyError::EnvelopeViolation("duty_cycle out of bounds"));
        }
        let z = (self.altitude_m)(&node.row.location);
        if z < self.z_min_m || z > self.z_max_m {
            return Err(SafetyError::EnvelopeViolation("altitude out of bounds"));
        }
        let s = node.row.ecoimpact_score;
        if s < self.ecoimpact_min || s > self.ecoimpact_max {
            return Err(SafetyError::EnvelopeViolation("ecoimpact score outside envelope"));
        }
        Ok(())
    }
}

/// Simple host budget over instantaneous power and per-step energy.
/// For full horizons, you can integrate externally and feed cumulative metrics here.
#[derive(Debug, Clone)]
pub struct SimpleHostBudget {
    pub p_max_w: f64,
    pub e_step_max_j: f64,
    /// Step duration in seconds for energy check.
    pub step_dt_s: f64,
}

impl HostBudget for SimpleHostBudget {
    fn check_host_budget(&self, node: &NodeState) -> Result<(), SafetyError> {
        if node.power_w > self.p_max_w {
            return Err(SafetyError::HostBudgetExceeded("instantaneous power exceeded"));
        }
        let e_step = node.power_w * self.step_dt_s;
        if e_step > self.e_step_max_j {
            return Err(SafetyError::HostBudgetExceeded("per-step energy exceeded"));
        }
        Ok(())
    }

    fn power_fraction(&self, node: &NodeState) -> f64 {
        if self.p_max_w <= 0.0 {
            0.0
        } else {
            (node.power_w / self.p_max_w).max(0.0)
        }
    }
}

/// Linear eco-band classifier based on corridor eco-load.
#[derive(Debug, Clone)]
pub struct ThresholdEcoBand {
    pub theta_green_amber: f64,
    pub theta_amber_red: f64,
    pub gain_green: f64,
    pub gain_amber: f64,
    pub gain_red: f64,
}

impl EcoBandClassifier for ThresholdEcoBand {
    fn classify(&self, eco_load: f64) -> EcoBand {
        if eco_load < self.theta_green_amber {
            EcoBand::Green
        } else if eco_load < self.theta_amber_red {
            EcoBand::Amber
        } else {
            EcoBand::Red
        }
    }

    fn band_gain(&self, band: EcoBand) -> f64 {
        match band {
            EcoBand::Green => self.gain_green,
            EcoBand::Amber => self.gain_amber,
            EcoBand::Red => self.gain_red,
        }
    }
}

/// DW ceiling invariant over mass flux density.
#[derive(Debug, Clone)]
pub struct SimpleDwCeiling {
    pub phi_dw_max: f64,
}

impl DwCeilingInvariant for SimpleDwCeiling {
    fn check_dw_ceiling(&self, phi_dw: f64) -> Result<(), SafetyError> {
        if phi_dw > self.phi_dw_max {
            Err(SafetyError::DwCeilingExceeded("dw ceiling violated"))
        } else {
            Ok(())
        }
    }

    fn dw_violation(&self, phi_dw: f64) -> f64 {
        if self.phi_dw_max <= 0.0 {
            0.0
        } else {
            ((phi_dw - self.phi_dw_max) / self.phi_dw_max).max(0.0)
        }
    }
}

/// Unified corridor controller composing the four semantics into a duty update.
#[derive(Debug, Clone)]
pub struct CorridorController<E, H, B, D>
where
    E: SafetyEnvelope,
    H: HostBudget,
    B: EcoBandClassifier,
    D: DwCeilingInvariant,
{
    pub envelope: E,
    pub host_budget: H,
    pub eco_band: B,
    pub dw_ceiling: D,
    // Reference scales from shard
    pub m_ref_kg: f64,
    pub k_ref_nb: f64,
    // Gains for Equation 5
    pub eta_m: f64,
    pub eta_k: f64,
    pub eta_w: f64,
    pub eta_b: f64,
    pub eta_p: f64,
    pub eta_dw: f64,
}

impl<E, H, B, D> CorridorController<E, H, B, D>
where
    E: SafetyEnvelope,
    H: HostBudget,
    B: EcoBandClassifier,
    D: DwCeilingInvariant,
{
    /// Compute corridor-wide eco-load from nodes.
    /// This is Equation 3: E_corr = a_M M_corr/M_ref + a_K K_corr/K_ref.
    pub fn eco_load(&self, nodes: &[NodeState], alpha_m: f64, alpha_k: f64) -> f64 {
        let m_sum: f64 = nodes.iter().map(|n| n.mass_kg).sum();
        let k_sum: f64 = nodes.iter().map(|n| n.karma_bytes).sum();
        let m_norm = if self.m_ref_kg > 0.0 {
            m_sum / self.m_ref_kg
        } else {
            0.0
        };
        let k_norm = if self.k_ref_nb > 0.0 {
            k_sum / self.k_ref_nb
        } else {
            0.0
        };
        alpha_m * m_norm + alpha_k * k_norm
    }

    /// Compute DW flux density for the corridor from raw in/out flows.
    /// Here flux is supplied externally (already aggregated).
    pub fn dw_flux_density(&self, phi_dw_raw: f64) -> f64 {
        phi_dw_raw
    }

    /// Update a single node's duty-cycle using Equation 5, after all checks.
    pub fn update_node_duty(
        &self,
        node: &mut NodeState,
        eco_band: EcoBand,
        phi_dw: f64,
    ) -> Result<(), SafetyError> {
        // Envelope and host-budget checks first.
        self.envelope.check_envelope(node)?;
        self.host_budget.check_host_budget(node)?;

        // Compute normalized components.
        let m_norm = if self.m_ref_kg > 0.0 {
            node.mass_kg / self.m_ref_kg
        } else {
            0.0
        };
        let k_norm = if self.k_ref_nb > 0.0 {
            node.karma_bytes / self.k_ref_nb
        } else {
            0.0
        };
        let w = node.geo_weight;
        let band_gain = self.eco_band.band_gain(eco_band);
        let p_frac = self.host_budget.power_fraction(node);
        let dw_violation = self.dw_ceiling.dw_violation(phi_dw);

        let mut u_new = node.duty_cycle
            + self.eta_m * m_norm
            + self.eta_k * k_norm
            + self.eta_w * w
            + self.eta_b * band_gain
            - self.eta_p * p_frac
            - self.eta_dw * dw_violation;

        // Project onto [0,1].
        if u_new < 0.0 {
            u_new = 0.0;
        } else if u_new > 1.0 {
            u_new = 1.0;
        }

        node.duty_cycle = u_new;
        Ok(())
    }
}
