#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Parameter vector x = [distance_from_hive_m,
///                       o3_concentration_ugm3,
///                       emf_intensity_vpm,
///                       duty_cycle]
pub type ParameterVector = [f64; 4];

/// A single half-space constraint a·x + b <= 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearConstraint {
    pub a: ParameterVector,
    pub b: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeerightsPolytope {
    pub constraints: Vec<LinearConstraint>,
}

impl BeerightsPolytope {
    /// A very conservative default box; real deployments should load
    /// site-specific constraints from a shard or config.
    pub fn default_conservative() -> Self {
        // Example constraints (a·x + b <= 0):
        // 1) distance_from_hive_m >= 50  ->  -x0 + 50 <= 0
        // 2) o3_concentration_ugm3 <= 80 ->  x1 - 80 <= 0
        // 3) emf_intensity_vpm <= 1.0    ->  x2 - 1.0 <= 0
        // 4) duty_cycle <= 0.3           ->  x3 - 0.3 <= 0
        let c1 = LinearConstraint {
            a: [-1.0, 0.0, 0.0, 0.0],
            b: 50.0,
        };
        let c2 = LinearConstraint {
            a: [0.0, 1.0, 0.0, 0.0],
            b: -80.0,
        };
        let c3 = LinearConstraint {
            a: [0.0, 0.0, 1.0, 0.0],
            b: -1.0,
        };
        let c4 = LinearConstraint {
            a: [0.0, 0.0, 0.0, 1.0],
            b: -0.3,
        };

        BeerightsPolytope {
            constraints: vec![c1, c2, c3, c4],
        }
    }

    /// Returns true if all a·x + b <= 0 are satisfied (within tolerance).
    pub fn is_inside(&self, x: &ParameterVector, tol: f64) -> bool {
        self.constraints.iter().all(|c| {
            let dot =
                c.a[0] * x[0] + c.a[1] * x[1] + c.a[2] * x[2] + c.a[3] * x[3] + c.b;
            dot <= tol
        })
    }
}

/// Raw environmental inputs to Beekarma.
#[derive(Debug, Clone)]
pub struct BeeEnvSample {
    pub distance_from_hive_m: f64,
    pub o3_ugm3: f64,
    pub aqhi: f64,
    pub pm25_ugm3: f64,
    pub emf_vpm: f64,
    pub pesticide_index: f64, // normalized 0–1 (from shard or API)
}

/// Hazard index configuration.
#[derive(Debug, Clone)]
pub struct HazardWeights {
    pub w_poll: f64,
    pub w_bio: f64,
    pub w_rf: f64,
    pub o3_ref_ugm3: f64,
    pub aqhi_ref: f64,
    pub pm25_ref_ugm3: f64,
    pub emf_ref_vpm: f64,
}

impl HazardWeights {
    pub fn default() -> Self {
        HazardWeights {
            w_poll: 0.5,
            w_bio: 0.3,
            w_rf: 0.2,
            o3_ref_ugm3: 80.0,
            aqhi_ref: 7.0,
            pm25_ref_ugm3: 25.0,
            emf_ref_vpm: 1.0,
        }
    }
}

/// Compute H_poll from normalized O3, AQHI, PM2.5.
/// Evidence shows higher O3, AQHI, and temperature correlate with bee mortality.[web:60][web:61]
pub fn compute_h_poll(env: &BeeEnvSample, cfg: &HazardWeights) -> f64 {
    let o3 = (env.o3_ugm3 / cfg.o3_ref_ugm3).min(2.0);
    let aqhi = (env.aqhi / cfg.aqhi_ref).min(2.0);
    let pm25 = (env.pm25_ugm3 / cfg.pm25_ref_ugm3).min(2.0);
    let raw = (o3 + aqhi + pm25) / 3.0;
    raw.min(1.0).max(0.0)
}

/// Compute H_bio from pesticide and metal proxies.[web:62][web:68]
pub fn compute_h_bio(env: &BeeEnvSample) -> f64 {
    env.pesticide_index.min(1.0).max(0.0)
}

/// Compute H_rf from EMF intensity.
pub fn compute_h_rf(env: &BeeEnvSample, cfg: &HazardWeights) -> f64 {
    let rf = (env.emf_vpm / cfg.emf_ref_vpm).min(2.0);
    rf.min(1.0).max(0.0)
}

/// Aggregate into H_bee in [0,1].
pub fn compute_h_bee(env: &BeeEnvSample, cfg: &HazardWeights) -> f64 {
    let h_poll = compute_h_poll(env, cfg);
    let h_bio = compute_h_bio(env);
    let h_rf = compute_h_rf(env, cfg);

    let num = cfg.w_poll * h_poll + cfg.w_bio * h_bio + cfg.w_rf * h_rf;
    let den = cfg.w_poll + cfg.w_bio + cfg.w_rf;
    (num / den).min(1.0).max(0.0)
}

/// Check bee rights and optionally downscale duty cycle.
/// This is the main function CyboAir should call before actuating.
pub fn enforce_bee_rights(
    env: &BeeEnvSample,
    proposed_duty_cycle: f64,
    polytope: &BeerightsPolytope,
) -> (bool, f64) {
    let dc_clamped = proposed_duty_cycle.clamp(0.0, 1.0);

    let x: ParameterVector = [
        env.distance_from_hive_m,
        env.o3_ugm3,
        env.emf_vpm,
        dc_clamped,
    ];

    if polytope.is_inside(&x, 1e-9) {
        (true, dc_clamped)
    } else {
        // Simple mitigation strategy:
        // - If too close or too polluted, drop duty cycle to a safe minimum.
        // In a full implementation, this should solve a small LP to project
        // back into the polytope, but here we enforce a hard de-rate.
        let safe_dc = 0.0;
        (false, safe_dc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polytope_inside_and_outside() {
        let p = BeerightsPolytope::default_conservative();

        // Clearly safe point.
        let x_safe: ParameterVector = [100.0, 40.0, 0.2, 0.1];
        assert!(p.is_inside(&x_safe, 1e-9));

        // Too close, too high duty cycle, excessive EMF and O3.
        let x_unsafe: ParameterVector = [10.0, 120.0, 2.0, 0.8];
        assert!(!p.is_inside(&x_unsafe, 1e-9));
    }

    #[test]
    fn test_hazard_indices_and_enforcement() {
        let env = BeeEnvSample {
            distance_from_hive_m: 60.0,
            o3_ugm3: 70.0,
            aqhi: 6.0,
            pm25_ugm3: 20.0,
            emf_vpm: 0.5,
            pesticide_index: 0.4,
        };
        let cfg = HazardWeights::default();
        let h_bee = compute_h_bee(&env, &cfg);
        assert!(h_bee >= 0.0 && h_bee <= 1.0);

        let poly = BeerightsPolytope::default_conservative();
        let (ok, dc) = enforce_bee_rights(&env, 0.2, &poly);
        assert!(ok);
        assert_eq!(dc, 0.2);

        let env_bad = BeeEnvSample {
            distance_from_hive_m: 20.0,
            o3_ugm3: 100.0,
            aqhi: 9.0,
            pm25_ugm3: 50.0,
            emf_vpm: 2.0,
            pesticide_index: 0.9,
        };
        let (ok2, dc2) = enforce_bee_rights(&env_bad, 0.8, &poly);
        assert!(!ok2);
        assert_eq!(dc2, 0.0);
    }
}
