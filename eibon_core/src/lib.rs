use std::error::Error;

/// Core row schema, aligned with Cybo-Air / EcoNet qpudatashards.
#[derive(Debug, Clone)]
pub struct GovernanceRow {
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

/// Deterministic unit operator C_u (kg/m3 per reported unit).
pub fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    match unit {
        "ug/m3" => 1e-9,
        "mg/m3" => 1e-6,
        "ppb" => {
            let r = 8.3145_f64;
            molar_mass_kg_per_mol / (r * temperature_k) * 1e-9
        }
        _ => 0.0,
    }
}

/// CEIM-style conserved mass operator Mx = Cu * Q * t.
pub fn compute_mass_kg(
    row: &GovernanceRow,
    temperature_k: f64,
    molar_mass_kg_per_mol: f64,
) -> f64 {
    let alpha = unit_to_kg_factor(&row.unit, temperature_k, molar_mass_kg_per_mol);
    let delta_c = (row.cin - row.cout).max(0.0);
    let c_u = alpha * delta_c;
    c_u * row.airflow_m3_per_s * row.period_s
}

/// Hazard-weighted NanoKarmaBytes Kx = lambda * beta * Mx.
pub fn compute_karma_bytes(row: &GovernanceRow, mass_kg: f64) -> f64 {
    row.lambda_hazard * row.beta_nb_per_kg * mass_kg
}

/// Normalized ecoimpact index Sx in [0,1].
pub fn compute_ecoimpact(mass_kg: f64, karma_bytes: f64, k0: f64, alpha: f64) -> f64 {
    if k0 <= 0.0 {
        return 0.0;
    }
    let k_ratio = karma_bytes / k0;
    1.0 - (-alpha * k_ratio).exp()
}

/// Governance check: ensure mass and Karma are physically plausible.
pub fn validate_row(
    row: &GovernanceRow,
    temperature_k: f64,
    molar_mass_kg_per_mol: f64,
) -> Result<(), Box<dyn Error>> {
    let m = compute_mass_kg(row, temperature_k, molar_mass_kg_per_mol);
    if m < 0.0 {
        return Err("Negative mass violates CEIM conservation".into());
    }
    if row.lambda_hazard < 0.0 || row.beta_nb_per_kg < 0.0 {
        return Err("Negative hazard or Karma/kg violates governance spec".into());
    }
    Ok(())
}
