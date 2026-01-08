use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
struct CyboAirRow {
    machine_id: String,
    r#type: String,
    location: String,
    pollutant: String,
    cin: f64,
    cout: f64,
    unit: String,
    airflow_m3_per_s: f64,
    period_s: f64,
    lambda_hazard: f64,
    beta_nb_per_kg: f64,
    ecoimpactscore: f64,
    notes: String,
}

#[derive(Debug, Clone)]
struct NodeState {
    row: CyboAirRow,
    mass_kg: f64,
    karma_bytes: f64,
    duty_cycle: f64,
}

fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
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

fn parse_csv_row(line: &str) -> Result<CyboAirRow, Box<dyn Error>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ',' if !in_quotes => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current.trim().to_string());
    }

    if parts.len() < 13 {
        return Err("Not enough columns".into());
    }

    Ok(CyboAirRow {
        machine_id: parts[0].clone(),
        r#type: parts[1].clone(),
        location: parts[2].clone(),
        pollutant: parts[3].clone(),
        cin: parts[4].parse()?,
        cout: parts[5].parse()?,
        unit: parts[6].clone(),
        airflow_m3_per_s: parts[7].parse()?,
        period_s: parts[8].parse()?,
        lambda_hazard: parts[9].parse()?,
        beta_nb_per_kg: parts[10].parse()?,
        ecoimpactscore: parts[11].parse()?,
        notes: parts[12].clone(),
    })
}

fn update_node(
    node: &mut NodeState,
    temperature_k: f64,
    molar_mass_kg_per_mol: f64,
    m_ref: f64,
    k_ref: f64,
    w_i: f64,
    c_power_i: f64,
    eta1: f64,
    eta2: f64,
    eta3: f64,
    eta4: f64,
) {
    let r = &node.row;
    let alpha = unit_to_kg_factor(&r.unit, temperature_k, molar_mass_kg_per_mol);
    let d_c = (r.cin - r.cout).max(0.0);
    node.mass_kg = d_c * alpha * r.airflow_m3_per_s * r.period_s;
    node.karma_bytes = r.lambda_hazard * r.beta_nb_per_kg * node.mass_kg;

    let u = node.duty_cycle
        + eta1 * (node.mass_kg / m_ref)
        + eta2 * (node.karma_bytes / k_ref)
        + eta3 * w_i
        - eta4 * c_power_i;

    node.duty_cycle = if u < 0.0 {
        0.0
    } else if u > 1.0 {
        1.0
    } else {
        u
    };
}

fn main() -> Result<(), Box<dyn Error>> {
    // Path: adjust to actual shard path
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
        let node = NodeState {
            row,
            mass_kg: 0.0,
            karma_bytes: 0.0,
            duty_cycle: 0.0,
        };
        nodes.push(node);
    }

    // Phoenix summer representative values for PM2.5, NOx, etc.
    let temperature_k = 310.0_f64;
    // Simplified; in production use per‑pollutant MW (e.g., O3, NO2, VOC surrogates)
    let molar_mass_kg_per_mol = 0.048_f64;

    // Reference scales derived from shard order of magnitude
    let m_ref = 1e-6_f64;
    let k_ref = 1e10_f64;

    // Control gains
    let eta1 = 0.1_f64;
    let eta2 = 0.1_f64;
    let eta3 = 0.2_f64;
    let eta4 = 0.05_f64;

    // Dummy geospatial and power weights for demonstration;
    // in deployment, compute from real gradients, clearances, and power telemetry.
    for node in &mut nodes {
        let w_i = if node.row.location.contains("School") {
            1.0
        } else if node.row.location.contains("Intersection") {
            0.8
        } else {
            0.5
        };
        let c_power_i = 0.3_f64;

        update_node(
            node,
            temperature_k,
            molar_mass_kg_per_mol,
            m_ref,
            k_ref,
            w_i,
            c_power_i,
            eta1,
            eta2,
            eta3,
            eta4,
        );
    }

    for node in &nodes {
        println!(
            "{}, location={}, pollutant={}, mass_kg={:.6e}, karma_bytes={:.6e}, duty_cycle={:.3}",
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
