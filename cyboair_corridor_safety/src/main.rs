use std::error::Error;

use cyboair_corridor_safety::{
    compute_karma_bytes, compute_mass_kg, CorridorController, CorridorRow, EcoBandClassifier,
    NodeState, RectSafetyEnvelope, SimpleDwCeiling, SimpleHostBudget, ThresholdEcoBand,
};

fn phoenix_altitude_m(_loc: &str) -> f64 {
    // Simple placeholder: Phoenix mean elevation ~ 331 m.
    // Replace with real DEM-based lookup in production.
    331.0
}

fn main() -> Result<(), Box<dyn Error>> {
    // Example: two nodes from a Phoenix-like shard.
    let row_canopy = CorridorRow {
        machine_id: "CYB-AIR-CANOPY-01".to_string(),
        r#type: "UrbanNanoswarmCanopy".to_string(),
        location: "Phoenix-Intersection-A".to_string(),
        pollutant: "PM2.5".to_string(),
        cin: 40.0,
        cout: 28.0,
        unit: "ugm3".to_string(),
        airflow_m3_per_s: 3.0,
        period_s: 3600.0,
        lambda_hazard: 3.0,
        beta_nb_per_kg: 5.0e8,
        ecoimpact_score: 0.92,
    };

    let row_school = CorridorRow {
        machine_id: "CYB-AIR-SCHOOL-05".to_string(),
        r#type: "SchoolZoneShield".to_string(),
        location: "Elementary-North".to_string(),
        pollutant: "PM2.5".to_string(),
        cin: 30.0,
        cout: 18.0,
        unit: "ugm3".to_string(),
        airflow_m3_per_s: 1.0,
        period_s: 2700.0,
        lambda_hazard: 4.0,
        beta_nb_per_kg: 5.5e8,
        ecoimpact_score: 0.94,
    };

    // Physics parameters (Phoenix summer).
    let temperature_k = 310.0_f64;
    let molar_mass_kg_per_mol = 0.048_f64; // surrogate for O3/NO2; for PM2.5 this is a proxy.

    let mut node_canopy = NodeState {
        row: row_canopy,
        mass_kg: 0.0,
        karma_bytes: 0.0,
        duty_cycle: 0.5,
        power_w: 50.0,
        geo_weight: 0.8,
    };

    let mut node_school = NodeState {
        row: row_school,
        mass_kg: 0.0,
        karma_bytes: 0.0,
        duty_cycle: 0.7,
        power_w: 35.0,
        geo_weight: 1.0,
    };

    // Populate mass and Karma using CEIM/NanoKarma operators.
    for node in [&mut node_canopy, &mut node_school] {
        let m = compute_mass_kg(&node.row, temperature_k, molar_mass_kg_per_mol);
        let k = compute_karma_bytes(&node.row, m);
        node.mass_kg = m;
        node.karma_bytes = k;
    }

    // Safety envelope: down-hanging urban band, high ecoimpact nodes.
    let envelope = RectSafetyEnvelope {
        u_min: 0.0,
        u_max: 1.0,
        z_min_m: 5.0,
        z_max_m: 600.0,
        ecoimpact_min: 0.7,
        ecoimpact_max: 1.0,
        altitude_m: phoenix_altitude_m,
    };

    // Host budgets (per node) — illustrative.
    let host_budget = SimpleHostBudget {
        p_max_w: 150.0,
        e_step_max_j: 1.0e5,
        step_dt_s: 300.0,
    };

    // Eco-band classifier: thresholds in normalized eco-load units.
    let eco_band = ThresholdEcoBand {
        theta_green_amber: 0.5,
        theta_amber_red: 1.0,
        gain_green: 0.0,
        gain_amber: 0.2,
        gain_red: 0.5,
    };

    // DW ceiling invariant: example maximum flux density.
    let dw_ceiling = SimpleDwCeiling { phi_dw_max: 1.0e-6 };

    // Reference scales from shard orders of magnitude.
    let controller = CorridorController {
        envelope,
        host_budget,
        eco_band,
        dw_ceiling,
        m_ref_kg: 1.0e-6,
        k_ref_nb: 1.0e10,
        eta_m: 0.1,
        eta_k: 0.1,
        eta_w: 0.2,
        eta_b: 0.2,
        eta_p: 0.05,
        eta_dw: 0.1,
    };

    // Corridor eco-load and band.
    let nodes_slice = [node_canopy.clone(), node_school.clone()];
    let eco_load = controller.eco_load(&nodes_slice, 0.5, 0.5);
    let band = controller.eco_band.classify(eco_load);

    // Example DW flux density (would be computed from in/out flows in production).
    let phi_dw = 5.0e-7; // below ceiling, no violation.

    // Update nodes.
    let mut nodes = vec![node_canopy, node_school];
    for node in nodes.iter_mut() {
        controller.update_node_duty(node, band, phi_dw)?;
    }

    // Emit control summary.
    for node in nodes.iter() {
        println!(
            "{},{},{},{:.6e},{:.6e},{:.3}",
            node.row.machine_id,
            node.row.location,
            node.row.pollutant,
            node.mass_kg,
            node.karma_bytes,
            node.duty_cycle
        );
    }

    Ok(())
}
