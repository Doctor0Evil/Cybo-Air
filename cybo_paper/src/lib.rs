#[derive(Debug, Clone)]
pub struct MineralSheet {
    pub area_m2: f64,
    pub e_prod_kgco2_per_m2: f64,
    pub m_carb_kg_per_m2: f64,
    pub beta_carb_kgco2_per_kg: f64,
}

impl MineralSheet {
    /// Net embodied CO₂ per m² after full carbonation potential.
    pub fn net_co2_kg_per_m2(&self) -> f64 {
        self.e_prod_kgco2_per_m2 - self.beta_carb_kgco2_per_kg * self.m_carb_kg_per_m2
    }
}
