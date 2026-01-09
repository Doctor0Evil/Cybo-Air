use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
struct CyboAirRow {
    machineid: String,
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
    ecoimpact_score: f64,
    notes: String,
}

#[derive(Debug, Clone)]
struct NodeState {
    row: CyboAirRow,
    mass_kg: f64,
    karma_bytes: f64,
    duty_cycle: f64,
}

/// Convert concentration units to kg/m^3 factor using Phoenix T and pollutant MW.
/// For PM (µg/m3, mg/m3), MW is not needed; for ppb gases, MW (kg/mol) is required.
fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    match unit {
        "ugm3" | "µg/m3" => 1e-9,
        "mgm3" | "mg/m3" => 1e-6,
        "ppb" => {
            let r = 8.3145_f64;
            (molar_mass_kg_per_mol / (r * temperature_k)) * 1e-9
        }
        _ => 0.0,
    }
}

/// Parse a CSV line into CyboAirRow. Assumes no embedded commas in unquoted fields.
fn parse_csv_row(line: &str) -> Result<CyboAirRow, Box<dyn Error>> {
    let mut parts: Vec<String> = Vec::new();
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
        return Err("Not enough columns in CyboAir row".into());
    }

    Ok(CyboAirRow {
        machineid: parts[0].clone(),
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
        ecoimpact_score: parts[11].parse()?,
        notes: parts[12].clone(),
    })
}

/// Compute mass, NanoKarma, and updated duty cycle for one node.
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
    alpha_eco: f64,
    k0_eco: f64,
) {
    let r = &node.row;
    let alpha = unit_to_kg_factor(&r.unit, temperature_k, molar_mass_kg_per_mol);
    let d_c = (r.cin - r.cout).max(0.0);
    let c_u = alpha * d_c;

    // CEIM mass operator
    node.mass_kg = c_u * r.airflow_m3_per_s * r.period_s;

    // Hazard-weighted NanoKarmaBytes
    node.karma_bytes = r.lambda_hazard * r.beta_nb_per_kg * node.mass_kg;

    // Optional: recompute ecoimpact_score from K (kept here for cross-check)
    let s_model = 1.0 - (-alpha_eco * (node.karma_bytes / k0_eco)).exp();
    let _s_combined = 0.5 * r.ecoimpact_score + 0.5 * s_model;

    // Duty-cycle control law with projection
    let mut u = node.duty_cycle
        + eta1 * (node.mass_kg / m_ref)
        + eta2 * (node.karma_bytes / k_ref)
        + eta3 * w_i
        - eta4 * c_power_i;

    if u < 0.0 {
        u = 0.0;
    } else if u > 1.0 {
        u = 1.0;
    }
    node.duty_cycle = u;
}

fn main() -> Result<(), Box<dyn Error>> {
    // Adjust path if needed
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
        });
    }

    // Phoenix‑representative parameters
    let temperature_k = 310.0_f64;
    // For simplicity, use one MW for gases here; in production this is per‑pollutant
    let molar_mass_kg_per_mol = 0.048_f64; // ~O3 surrogate

    // Reference scales from shard orders of magnitude
    let m_ref = 1e-6_f64;    // 1 mg captured
    let k_ref = 1e10_f64;    // 1e10 NanoKarmaBytes
    let alpha_eco = 1.0_f64; // ecoimpact nonlinearity
    let k0_eco = 1e9_f64;    // scaling for S(K)

    // Control gains
    let eta1 = 0.1_f64;
    let eta2 = 0.1_f64;
    let eta3 = 0.2_f64;
    let eta4 = 0.05_f64;

    // One control update step; in deployment, run this in a loop
    for node in nodes.iter_mut() {
        // Simple geospatial weight: prioritize schools, then intersections
        let w_i = if node.row.location.contains("School")
            || node.row.location.contains("Elementary")
        {
            1.0
        } else if node.row.location.contains("Intersection")
            || node.row.location.contains("Industrial")
        {
            0.8
        } else {
            0.5
        };

        // Normalized power cost: more cost for high airflow machines
        let c_power_i = (node.row.airflow_m3_per_s / 3.0).min(1.0);

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
            alpha_eco,
            k0_eco,
        );
    }

    // Print control‑relevant summary for all five machine classes
    println!(
        "machineid,location,type,pollutant,mass_kg,karma_bytes,duty_cycle"
    );
    for node in nodes.iter() {
        println!(
            "{},{},{},{},{:.6e},{:.6e},{:.3}",
            node.row.machineid,
            node.row.location,
            node.row.r#type,
            node.row.pollutant,
            node.mass_kg,
            node.karma_bytes,
            node.duty_cycle
        );
    }

    Ok(())
}
