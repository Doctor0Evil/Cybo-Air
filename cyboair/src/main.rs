use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
struct CyboAirRow {
    machineid: String,
    rtype: String,
    location: String,
    pollutant: String,
    cin: f64,
    cout: f64,
    unit: String,
    airflow_m3_per_s: f64,
    period_s: f64,
    lambda_hazard: f64,
    beta_nb_per_kg: f64,
    ecoimpact_score: f64,
    notes: String,
}

#[derive(Debug, Clone)]
struct NodeState {
    row: CyboAirRow,
    mass_kg: f64,
    karma_bytes: f64,
    duty_cycle: f64,
    w_geo: f64,
    c_power: f64,
}

/// Eq. 1: unit operator u(unit, T, MW) -> kg/m^3 per unit
fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    match unit {
        "ugm3" => 1e-9,
        "mgm3" => 1e-6,
        "ppb" => {
            let r = 8.3145_f64;
            molar_mass_kg_per_mol / (r * temperature_k) * 1e-9
        }
        _ => 0.0,
    }
}

/// Parse one CSV line from CyboAirTenMachinesPhoenix2026v1.csv
fn parse_csv_row(line: &str) -> Result<CyboAirRow, Box<dyn Error>> {
    // Simple CSV split; assumes no embedded commas in unquoted fields
    let parts: Vec<String> = line
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    if parts.len() < 13 {
        return Err("Not enough columns in CyboAir row".into());
    }

    Ok(CyboAirRow {
        machineid: parts[0].clone(),
        rtype: parts[1].clone(),
        location: parts[2].clone(),
        pollutant: parts[3].clone(),
        cin: parts[4].parse()?,
        cout: parts[5].parse()?,
        unit: parts[6].clone(),
        airflow_m3_per_s: parts[7].parse()?,
        period_s: parts[8].parse()?,
        lambda_hazard: parts[9].parse()?,
        beta_nb_per_kg: parts[10].parse()?,
        ecoimpact_score: parts[11].parse()?,
        notes: parts[12].clone(),
    })
}

/// Compute simple geospatial weight w_i (Eq. 4, here using location tags)
fn compute_w_geo(location: &str) -> f64 {
    if location.contains("School") {
        1.0
    } else if location.contains("Intersection") || location.contains("BusRoute") {
        0.8
    } else if location.contains("Canal") || location.contains("Farm") {
        0.7
    } else {
        0.5
    }
}

/// Single-node update implementing Eqs. 1–5
fn update_node(
    node: &mut NodeState,
    temperature_k: f64,
    molar_mass_kg_per_mol: f64,
    m_ref: f64,
    k_ref: f64,
    eta1: f64,
    eta2: f64,
    eta3: f64,
    eta4: f64,
) {
    let r = &node.row;

    // Eq. 1: mass removed M_i
    let delta_c = (r.cin - r.cout).max(0.0);
    let alpha = unit_to_kg_factor(&r.unit, temperature_k, molar_mass_kg_per_mol);
    let c_u = alpha * delta_c;
    node.mass_kg = c_u * r.airflow_m3_per_s * r.period_s;

    // Eq. 2: NanoKarmaBytes K_i
    node.karma_bytes = r.lambda_hazard * r.beta_nb_per_kg * node.mass_kg;

    // Eq. 4: geospatial weight w_i (here fixed from location)
    node.w_geo = compute_w_geo(&r.location);

    // Example power-normalized term c_power,i (constant here; in deployment from telemetry)
    node.c_power = 0.3;

    // Eq. 5: duty cycle update with projection onto [0,1]
    let m_term = if m_ref > 0.0 { node.mass_kg / m_ref } else { 0.0 };
    let k_term = if k_ref > 0.0 { node.karma_bytes / k_ref } else { 0.0 };

    let mut u_next = node.duty_cycle
        + eta1 * m_term
        + eta2 * k_term
        + eta3 * node.w_geo
        - eta4 * node.c_power;

    if u_next < 0.0 {
        u_next = 0.0;
    } else if u_next > 1.0 {
        u_next = 1.0;
    }
    node.duty_cycle = u_next;
}

fn main() -> Result<(), Box<dyn Error>> {
    // Path to Phoenix shard (adjust as needed)
    let file = File::open("qpudatashards/particles/CyboAirTenMachinesPhoenix2026v1.csv")?;
    let reader = BufReader::new(file);

    let mut lines = reader.lines();

    // Skip header
    let _header = lines.next();

    let mut nodes: Vec<NodeState> = Vec::new();

    for line_res in lines {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }
        let row = parse_csv_row(&line)?;
        nodes.push(NodeState {
            row,
            mass_kg: 0.0,
            karma_bytes: 0.0,
            duty_cycle: 0.0,
            w_geo: 0.0,
            c_power: 0.0,
        });
    }

    // Phoenix-like defaults; for real deployment, use pollutant-specific MW_x
    let temperature_k = 310.0_f64;
    let molar_mass_kg_per_mol = 0.048_f64; // e.g., O3 / VOC surrogate

    // Reference scales from shard order-of-magnitude
    let m_ref = 1e-6_f64;
    let k_ref = 1e10_f64;

    // Control gains
    let eta1 = 0.1_f64;
    let eta2 = 0.1_f64;
    let eta3 = 0.2_f64;
    let eta4 = 0.05_f64;

    // Single control step over all nodes
    for node in &mut nodes {
        update_node(
            node,
            temperature_k,
            molar_mass_kg_per_mol,
            m_ref,
            k_ref,
            eta1,
            eta2,
            eta3,
            eta4,
        );
    }

    // Governance-grade log of mass, Karma, and duty cycle
    for node in &nodes {
        println!(
            "{:>18} | {:>24} | {:>8} | M={:10.3e} kg | K={:10.3e} Bytes | u={:5.3}",
            node.row.machineid,
            node.row.location,
            node.row.pollutant,
            node.mass_kg,
            node.karma_bytes,
            node.duty_cycle
        );
    }

    Ok(())
}
