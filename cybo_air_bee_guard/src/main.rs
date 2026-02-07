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
    ecoimpact_score: f64,
    bee_flag: u8,      // 1 if in bee foraging microspace
    bee_weight: f64,   // additional hazard multiplier for bees
    notes: String,
}

#[derive(Debug, Clone)]
struct NodeState {
    row: CyboAirRow,
    mass_kg: f64,
    karma_bee: f64,
    duty_cycle: f64,
    emf_score: f64,
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

    if parts.len() < 14 {
        return Err("Not enough columns in CyboAir+Bee row".into());
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
        ecoimpact_score: parts[11].parse()?,
        bee_flag: parts[12].parse()?,
        bee_weight: parts[13].parse()?,
        notes: if parts.len() > 14 { parts[14..].join(",") } else { String::new() },
    })
}

fn update_node_bee(
    node: &mut NodeState,
    temperature_k: f64,
    molar_mass_kg_per_mol: f64,
    m_ref: f64,
    k_ref: f64,
    e_ref_bee: f64,
    eta1: f64,
    eta2: f64,
    eta3: f64,
    eta4: f64,
) {
    let r = &node.row;
    let alpha = unit_to_kg_factor(&r.unit, temperature_k, molar_mass_kg_per_mol);
    let d_c = (r.cin - r.cout).max(0.0);
    let c_u = alpha * d_c;

    // CEIM-style mass for this pollutant
    node.mass_kg = c_u * r.airflow_m3_per_s * r.period_s;

    // Bee-weighted hazard: existing lambda * bee_weight if flag set
    let lambda_bee = if node.row.bee_flag == 1 {
        r.lambda_hazard * r.bee_weight
    } else {
        r.lambda_hazard
    };

    node.karma_bee = lambda_bee * r.beta_nb_per_kg * node.mass_kg;

    // Simple local EMF score proxy: treat high-airflow nodes in bee zones as more EMF-sensitive
    node.emf_score = if node.row.bee_flag == 1 {
        (r.airflow_m3_per_s / 3.0).min(1.0)
    } else {
        0.0
    };

    // Bee-aware geospatial weight (simplified)
    let mut w_bee = 0.5;
    if node.row.bee_flag == 1 {
        w_bee += 0.3;
    }
    if node.row.location.contains("School")
        || node.row.location.contains("Orchard")
        || node.row.location.contains("Garden")
    {
        w_bee += 0.2;
    }
    w_bee -= (node.emf_score / e_ref_bee).min(0.5);

    // Duty-cycle update with projection to [0,1]
    let mut u = node.duty_cycle
        + eta1 * (node.mass_kg / m_ref)
        + eta2 * (node.karma_bee / k_ref)
        + eta3 * w_bee
        - eta4 * node.emf_score;

    if u < 0.0 {
        u = 0.0;
    } else if u > 1.0 {
        u = 1.0;
    }
    node.duty_cycle = u;
}

fn main() -> Result<(), Box<dyn Error>> {
    // Adjust path to your extended shard with bee columns
    let file = File::open("qpudatashards/particles/CyboAirTenMachinesPhoenix2026v1_bee.csv")?;
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
            karma_bee: 0.0,
            duty_cycle: 0.0,
            emf_score: 0.0,
        });
    }

    // Representative parameters (Phoenix summer, ozone surrogate MW)
    let temperature_k = 310.0_f64;
    let molar_mass_kg_per_mol = 0.048_f64;
    let m_ref = 1e-6_f64;
    let k_ref = 1e10_f64;
    let e_ref_bee = 1.0_f64;

    // Control gains
    let eta1 = 0.1_f64;
    let eta2 = 0.1_f64;
    let eta3 = 0.2_f64;
    let eta4 = 0.05_f64;

    // Single update step; in deployment, this runs in a loop
    for node in nodes.iter_mut() {
        update_node_bee(
            node,
            temperature_k,
            molar_mass_kg_per_mol,
            m_ref,
            k_ref,
            e_ref_bee,
            eta1,
            eta2,
            eta3,
            eta4,
        );
    }

    println!("machine_id,location,type,pollutant,mass_kg,karma_bee,duty_cycle,emf_score");
    for node in nodes.iter() {
        println!(
            "{},{},{},{},{:.6e},{:.6e},{:.3},{:.3}",
            node.row.machine_id,
            node.row.location,
            node.row.r#type,
            node.row.pollutant,
            node.mass_kg,
            node.karma_bee,
            node.duty_cycle,
            node.emf_score,
        );
    }

    Ok(())
}
