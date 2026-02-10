#![no_std]

pub struct QpuRow {
    pub node_id: [u8; 16],
    pub region: [u8; 16],
    pub band: u8,
    pub pollutant: u8, // enum index
    pub cin: f32,      // kg/m^3
    pub cout: f32,     // kg/m^3
    pub q: f32,        // m^3/s
    pub area: f32,     // m^2
    pub beta_band: f32,
    pub dt: f32,       // s
    pub lambda_hazard: f32,
    pub beta_nb_per_kg: f32,
}

pub struct NodeTelemetry {
    pub m_removed_kg: f32,
    pub nk_bytes: f32,
    pub ecoimpact_score: f32,
    pub duty_next: f32,
}

pub fn step_node(rows: &[QpuRow], u_k: f32, p_watts: f32,
                 eta_cost: f32, gamma_mass: f32) -> NodeTelemetry {
    let mut m_total = 0.0f32;
    let mut k_total = 0.0f32;
    for r in rows {
        let m = (r.cin - r.cout) * r.q * r.dt; // kg
        let j = m / (r.area * r.dt).max(1e-6); // kg m^-2 s^-1
        let m_band = j * r.area * r.beta_band * r.dt;
        let k = r.beta_nb_per_kg * r.lambda_hazard * m_band;
        m_total += m_band;
        k_total += k;
    }
    let grad = gamma_mass * m_total - eta_cost * p_watts;
    let mut u_next = u_k + grad;
    if u_next < 0.0 { u_next = 0.0; }
    if u_next > 1.0 { u_next = 1.0; }
    NodeTelemetry {
        m_removed_kg: m_total,
        nk_bytes: k_total,
        ecoimpact_score: k_total, // normalized upstream in CEIM
        duty_next: u_next,
    }
}
