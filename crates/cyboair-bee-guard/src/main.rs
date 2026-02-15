use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
struct CyboAirRow {
    machine_id: String,
    rtype: String,
    location: String,
    pollutant: String,
    c_in: f64,
    c_out: f64,
    unit: String,
    airflow_m3_per_s: f64,
    dt_s: f64,
    lambda_hazard: f64,
    beta_nb_per_kg: f64,
}

#[derive(Debug, Clone)]
struct BeeContext {
    hive_id: String,
    colony_mass_kg: f64,
    colony_mass_baseline_kg: f64,
    sbee_min: f64,
    kref_bee: f64,
    alpha: f64,
}

#[derive(Debug, Clone)]
struct NodeState {
    row: CyboAirRow,
    mass_kg: f64,
    air_karma_bytes: f64,
    bee_karma_bytes: f64,
    duty_cycle: f64,
    sbee: f64,
}

// Unit conversion to kg/m3 equivalent
fn unit_to_kg_factor(unit: &str, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    match unit {
        "ug/m3" => 1e-9,
        "mg/m3" => 1e-6,
        "ppb" => {
            // Ideal gas: ρ = (p * M) / (R*T); assume 1 atm, convert ppb by volume to kg/m3
            let r = 8.3145_f64;
            let p_pa = 101_325.0_f64;
            let base = p_pa * molar_mass_kg_per_mol / (r * temperature_k);
            base * 1e-9
        }
        _ => 0.0,
    }
}

// Parse a simple CSV with no embedded commas in fields
fn parse_csv_row(line: &str) -> Result<CyboAirRow, Box<dyn Error>> {
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.len() < 11 {
        return Err("Not enough columns in CSV row".into());
    }
    Ok(CyboAirRow {
        machine_id: parts[0].to_string(),
        rtype: parts[1].to_string(),
        location: parts[2].to_string(),
        pollutant: parts[3].to_string(),
        c_in: parts[4].parse()?,
        c_out: parts[5].parse()?,
        unit: parts[6].to_string(),
        airflow_m3_per_s: parts[7].parse()?,
        dt_s: parts[8].parse()?,
        lambda_hazard: parts[9].parse()?,
        beta_nb_per_kg: parts[10].parse()?,
    })
}

// Eq. 2 mass balance: M_j,h
fn compute_mass_kg(row: &CyboAirRow, temperature_k: f64, molar_mass_kg_per_mol: f64) -> f64 {
    let alpha = unit_to_kg_factor(&row.unit, temperature_k, molar_mass_kg_per_mol);
    let dc = (row.c_in - row.c_out).max(0.0);
    dc * alpha * row.airflow_m3_per_s * row.dt_s
}

// Existing air NanoKarma for compatibility
fn compute_air_karmabytes(row: &CyboAirRow, mass_kg: f64) -> f64 {
    row.lambda_hazard * row.beta_nb_per_kg * mass_kg
}

// Bee-specific hazard tables (λ_bee,j and β_bee,j)
fn bee_lambda_for_pollutant(p: &str) -> f64 {
    match p {
        "PM2.5" => 3.0,
        "VOC" => 4.0,
        "O3" => 2.5,
        "NOx" => 2.5,
        "DustPM10" => 1.5,
        _ => 1.0,
    }
}

fn bee_beta_for_pollutant(p: &str) -> f64 {
    match p {
        "PM2.5" => 6.0e8,
        "VOC" => 7.0e8,
        "O3" => 4.0e8,
        "NOx" => 4.0e8,
        "DustPM10" => 3.0e8,
        _ => 2.0e8,
    }
}

// Eq. 3 bee-specific karma K_bee,j
fn compute_bee_karmabytes(mass_kg: f64, lambda_bee_j: f64, beta_bee_j: f64) -> f64 {
    lambda_bee_j * beta_bee_j * mass_kg
}

// Eq. 4 normalized S_bee
fn compute_sbee(bee_karma_tot: f64, kref_bee: f64, alpha: f64) -> f64 {
    let denom = kref_bee.max(1.0);
    let x = -alpha * (bee_karma_tot / denom);
    let x_clip = x.max(-50.0).min(50.0);
    1.0 - f64::exp(x_clip)
}

// Residual-risk constraint (EFSA-style)
fn residual_risk_ok(ctx: &BeeContext) -> bool {
    let delta = (ctx.colony_mass_baseline_kg - ctx.colony_mass_kg)
        / ctx.colony_mass_baseline_kg.max(1e-6);
    delta <= 0.10
}

// Simple geospatial weight
fn geo_weight(location: &str) -> f64 {
    if location.contains("Apiary") || location.contains("School") {
        1.0
    } else if location.contains("Industrial") {
        0.8
    } else {
        0.5
    }
}

// Eq. 6 bee-aware duty-cycle update
fn update_duty_cycle(
    node: &mut NodeState,
    beectx: &BeeContext,
    mref: f64,
    kref: f64,
    cpower_i: f64,
    eta1: f64,
    eta2: f64,
    eta3: f64,
    eta4: f64,
    eta5: f64,
) {
    let mut phi_bee = 0.0;
    if residual_risk_ok(beectx) && node.sbee >= beectx.sbee_min {
        phi_bee = 1.0;
    } else if !residual_risk_ok(beectx) || node.sbee < beectx.sbee_min {
        phi_bee = -1.0;
    }

    let wi = geo_weight(&node.row.location);

    let uraw = node.duty_cycle
        + eta1 * (node.mass_kg / mref.max(1e-12))
        + eta2 * (node.air_karma_bytes / kref.max(1.0))
        + eta3 * wi
        + eta4 * phi_bee
        - eta5 * cpower_i;

    node.duty_cycle = if uraw <= 0.0 {
        0.0
    } else if uraw >= 1.0 {
        1.0
    } else {
        uraw
    };
}

fn main() -> Result<(), Box<dyn Error>> {
    // Adjust CSV path and schema to your deployment
    let file = File::open("data/cyboair_nodes_hive_corridor.csv")?;
    let reader = BufReader::new(file);

    let mut nodes: Vec<NodeState> = Vec::new();
    for (idx, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        if idx == 0 {
            // skip header
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let row = parse_csv_row(&line)?;
        nodes.push(NodeState {
            row,
            mass_kg: 0.0,
            air_karma_bytes: 0.0,
            bee_karma_bytes: 0.0,
            duty_cycle: 0.0,
            sbee: 1.0,
        });
    }

    // Example hive context; in production, derive from real hive telemetry
    let beectx = BeeContext {
        hive_id: "HIVE-PHX-01".to_string(),
        colony_mass_kg: 20.0,
        colony_mass_baseline_kg: 22.0,
        sbee_min: 0.8,
        kref_bee: 1.0e12,
        alpha: 1.0,
    };

    let temperature_k = 310.0_f64;
    let molar_mass_kg_per_mol = 0.048_f64;

    // Reference scales and gains
    let mref = 1e-6_f64;
    let kref = 1e10_f64;
    let eta1 = 0.1_f64;
    let eta2 = 0.1_f64;
    let eta3 = 0.2_f64;
    let eta4 = 0.2_f64;
    let eta5 = 0.05_f64;
    let cpower_i = 0.3_f64;

    // First pass: mass and karma per node
    for node in nodes.iter_mut() {
        node.mass_kg = compute_mass_kg(&node.row, temperature_k, molar_mass_kg_per_mol);
        node.air_karma_bytes = compute_air_karmabytes(&node.row, node.mass_kg);
        let lambda_bee = bee_lambda_for_pollutant(&node.row.pollutant);
        let beta_bee = bee_beta_for_pollutant(&node.row.pollutant);
        node.bee_karma_bytes = compute_bee_karmabytes(node.mass_kg, lambda_bee, beta_bee);
    }

    // Aggregate bee karma across nodes in the hive corridor
    let bee_karma_tot: f64 = nodes.iter().map(|n| n.bee_karma_bytes).sum();
    let sbee = compute_sbee(bee_karma_tot, beectx.kref_bee, beectx.alpha);

    // Second pass: propagate S_bee and update duty cycles
    for node in nodes.iter_mut() {
        node.sbee = sbee;
        update_duty_cycle(
            node,
            &beectx,
            mref,
            kref,
            cpower_i,
            eta1,
            eta2,
            eta3,
            eta4,
            eta5,
        );
    }

    // Telemetry output
    println!(
        "machine_id,location,pollutant,mass_kg,air_karmabytes,bee_karmabytes,sbee,duty_cycle"
    );
    for node in nodes.iter() {
        println!(
            "{},{},{},{:.6e},{:.6e},{:.6e},{:.3},{:.3}",
            node.row.machine_id,
            node.row.location,
            node.row.pollutant,
            node.mass_kg,
            node.air_karma_bytes,
            node.bee_karma_bytes,
            node.sbee,
            node.duty_cycle
        );
    }

    Ok(())
}
