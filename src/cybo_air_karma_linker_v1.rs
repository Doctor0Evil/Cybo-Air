use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};

/// Unique identifier for a Cybo-Air node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AirNodeId(pub String);

/// Air pollutant type tracked by Cybo-Air.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AirPollutant {
    Pm25,
    No2,
    O3,
    Voc,
    BlackCarbon,
    Other(String),
}

impl AirPollutant {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "PM2.5" | "PM_2_5" | "PM25" => AirPollutant::Pm25,
            "NO2" | "NO_2" => AirPollutant::No2,
            "O3" | "OZONE" => AirPollutant::O3,
            "VOC" | "VOCs" | "VOCS" => AirPollutant::Voc,
            "BC" | "BLACK_CARBON" | "BLACKCARBON" => AirPollutant::BlackCarbon,
            other => AirPollutant::Other(other.to_string()),
        }
    }
}

/// Concentration unit for air pollutants.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AirConcentrationUnit {
    UgPerM3,
    Ppb,
    Ppm,
    Other(String),
}

impl AirConcentrationUnit {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "ug/m3" | "µg/m3" | "ug/m^3" => AirConcentrationUnit::UgPerM3,
            "ppb" | "PPB" => AirConcentrationUnit::Ppb,
            "ppm" | "PPM" => AirConcentrationUnit::Ppm,
            other => AirConcentrationUnit::Other(other.to_string()),
        }
    }
}

/// Volumetric air-flow unit for Cybo-Air devices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AirFlowUnit {
    M3PerS,
    M3PerH,
    Other(String),
}

impl AirFlowUnit {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "m3/s" => AirFlowUnit::M3PerS,
            "m3/h" | "m3/hr" => AirFlowUnit::M3PerH,
            other => AirFlowUnit::Other(other.to_string()),
        }
    }
}

/// Node-level eco-impact configuration and baseline conditions.
#[derive(Debug, Clone)]
pub struct CyboAirNodeMeta {
    pub node_id: AirNodeId,
    /// Human-readable description (e.g., "Phoenix arterial canopy").
    pub label: String,
    pub pollutant: AirPollutant,
    pub cin_baseline: f64,
    pub cin_unit: AirConcentrationUnit,
    /// Reference concentration C_ref (standard or guideline).
    pub cref: f64,
    pub cref_unit: AirConcentrationUnit,
    pub q_air: f64,
    pub q_unit: AirFlowUnit,
    /// Time horizon [s] used for eco-impact accumulation.
    pub horizon_s: f64,
    /// EcoNet-style normalized ecoimpact score in [0,1].
    pub ecoimpactscore: f64,
    /// Hazard weight λ_x for this pollutant type.
    pub hazard_weight: f64,
    /// Karma conversion factor per canonical impact unit.
    pub karma_per_unit: f64,
    /// Arbitrary notes, suitable for governance logs.
    pub notes: String,
}

/// Canonical impact and NanoKarma result for a Cybo-Air node.
#[derive(Debug, Clone)]
pub struct CyboAirImpact {
    /// Mass removed M_x (concentration difference × flow × time).
    pub mass_removed: f64,
    /// Medium-agnostic canonical impact (dimensionless).
    pub canonical_impact: f64,
    /// NanoKarmaBytes awarded for this operation window.
    pub nano_karma_bytes: f64,
}

/// Errors when parsing Cybo-Air qpudatashards.
#[derive(Debug)]
pub enum CyboAirShardError {
    Io(std::io::Error),
    Parse(String),
}

impl fmt::Display for CyboAirShardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CyboAirShardError::Io(e) => write!(f, "IO error: {}", e),
            CyboAirShardError::Parse(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl Error for CyboAirShardError {}

impl From<std::io::Error> for CyboAirShardError {
    fn from(err: std::io::Error) -> Self {
        CyboAirShardError::Io(err)
    }
}

/// Minimal CSV splitter that supports quoted fields.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ',' if !in_quotes => {
                fields.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() || line.ends_with(',') {
        fields.push(current.trim().to_string());
    }
    fields
}

/// Convert a volumetric air-flow Q to m3/s.
fn normalize_flow_to_m3_per_s(q: f64, unit: &AirFlowUnit) -> f64 {
    match unit {
        AirFlowUnit::M3PerS => q,
        AirFlowUnit::M3PerH => q / 3600.0,
        AirFlowUnit::Other(_) => q, // Assume upstream conversion if unknown.
    }
}

/// Load Cybo-Air qpudatashard CSV into node metadata structures.
///
/// Expected column order (example, can be adapted by callers):
/// node_id,label,pollutant,cin_baseline,cin_unit,cref,cref_unit,q_air,q_unit,horizon_s,ecoimpactscore,hazard_weight,karma_per_unit,notes
pub fn load_cyboair_nodes_from_csv(path: &str) -> Result<Vec<CyboAirNodeMeta>, CyboAirShardError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut lines = reader.lines();
    // Skip header
    let _header = match lines.next() {
        Some(Ok(h)) => h,
        Some(Err(e)) => return Err(CyboAirShardError::Io(e)),
        None => return Ok(Vec::new()),
    };

    let mut nodes = Vec::new();

    for (idx, line_res) in lines.enumerate() {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_csv_line(&line);
        if fields.len() < 14 {
            return Err(CyboAirShardError::Parse(format!(
                "Line {} has insufficient fields: {}",
                idx + 2,
                fields.len()
            )));
        }

        let node_id = AirNodeId(fields[0].to_string());
        let label = fields[1].to_string();
        let pollutant = AirPollutant::from_str(&fields[2]);

        let cin_baseline: f64 = fields[3]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("cin_baseline parse error: {}", e)))?;
        let cin_unit = AirConcentrationUnit::from_str(&fields[4]);

        let cref: f64 = fields[5]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("cref parse error: {}", e)))?;
        let cref_unit = AirConcentrationUnit::from_str(&fields[6]);

        let q_air: f64 = fields[7]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("q_air parse error: {}", e)))?;
        let q_unit = AirFlowUnit::from_str(&fields[8]);

        let horizon_s: f64 = fields[9]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("horizon_s parse error: {}", e)))?;

        let ecoimpactscore: f64 = fields[10]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("ecoimpactscore parse error: {}", e)))?;

        let hazard_weight: f64 = fields[11]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("hazard_weight parse error: {}", e)))?;

        let karma_per_unit: f64 = fields[12]
            .parse()
            .map_err(|e| CyboAirShardError::Parse(format!("karma_per_unit parse error: {}", e)))?;

        let notes = fields[13..].join(",");

        nodes.push(CyboAirNodeMeta {
            node_id,
            label,
            pollutant,
            cin_baseline,
            cin_unit,
            cref,
            cref_unit,
            q_air,
            q_unit,
            horizon_s,
            ecoimpactscore,
            hazard_weight,
            karma_per_unit,
            notes,
        });
    }

    Ok(nodes)
}

/// Compute pollutant mass removal for a Cybo-Air node over its horizon.
///
/// M_x = (C_in - C_out) * Q * t
///
/// For air, this function operates in "concentration units × m3 × s".
/// If absolute SI mass is required, a separate pollutant-specific converter
/// can be applied by the caller using molecular weight or density data.
pub fn compute_cyboair_mass_removed(
    cin: f64,
    cout: f64,
    q_air_m3_per_s: f64,
    horizon_s: f64,
) -> f64 {
    let delta_c = (cin - cout).max(0.0);
    delta_c * q_air_m3_per_s * horizon_s
}

/// Compute canonical impact and NanoKarmaBytes for a Cybo-Air node.
///
/// K_x = λ_x * ∫ (C_in - C_out) / C_ref * Q dt
/// Here, we approximate the integral over a fixed horizon:
/// K_x ≈ λ_x * (C_in - C_out) / C_ref * Q * t
pub fn evaluate_cyboair_impact(
    meta: &CyboAirNodeMeta,
    cout: f64,
) -> CyboAirImpact {
    let q_m3_s = normalize_flow_to_m3_per_s(meta.q_air, &meta.q_unit);
    let mass_removed = compute_cyboair_mass_removed(
        meta.cin_baseline,
        cout,
        q_m3_s,
        meta.horizon_s,
    );

    let cref = if meta.cref > 0.0 { meta.cref } else { 1.0 };
    let delta_c_norm = ((meta.cin_baseline - cout).max(0.0)) / cref;

    let canonical_impact = meta.hazard_weight * delta_c_norm * q_m3_s * meta.horizon_s;
    let nano_karma_bytes =
        canonical_impact * meta.ecoimpactscore.clamp(0.0, 1.0) * meta.karma_per_unit;

    CyboAirImpact {
        mass_removed,
        canonical_impact,
        nano_karma_bytes,
    }
}

/// Aggregated evaluation over multiple nodes, useful for city-wide scenarios.
pub fn evaluate_cyboair_system_impact(
    nodes: &[CyboAirNodeMeta],
    couts: &[f64],
) -> (f64, f64, f64) {
    assert_eq!(nodes.len(), couts.len());

    let mut total_mass = 0.0;
    let mut total_canonical = 0.0;
    let mut total_karma = 0.0;

    for (meta, &cout) in nodes.iter().zip(couts.iter()) {
        let impact = evaluate_cyboair_impact(meta, cout);
        total_mass += impact.mass_removed;
        total_canonical += impact.canonical_impact;
        total_karma += impact.nano_karma_bytes;
    }

    (total_mass, total_canonical, total_karma)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_csv_line_quotes() {
        let line = "ID1,\"Phoenix arterial canopy\",PM2.5,25.0,ug/m3,10.0,ug/m3,0.2,m3/s,3600,0.9,2.0,1.0e6,\"High priority\"";
        let f = split_csv_line(line);
        assert_eq!(f[0], "ID1");
        assert_eq!(f[1], "Phoenix arterial canopy");
        assert_eq!(f[2], "PM2.5");
        assert_eq!(f[13], "High priority");
    }

    #[test]
    fn test_compute_cyboair_mass_removed() {
        let cin = 25.0;
        let cout = 15.0;
        let q = 0.1;
        let t = 3600.0;
        let m = compute_cyboair_mass_removed(cin, cout, q, t);
        // (25 - 15) * 0.1 * 3600 = 10 * 360 = 3600
        assert!((m - 3600.0).abs() < 1e-6);
    }

    #[test]
    fn test_evaluate_cyboair_impact() {
        let meta = CyboAirNodeMeta {
            node_id: AirNodeId("PHX-ARTERIAL-01".to_string()),
            label: "Phoenix arterial canopy".to_string(),
            pollutant: AirPollutant::Pm25,
            cin_baseline: 25.0,
            cin_unit: AirConcentrationUnit::UgPerM3,
            cref: 10.0,
            cref_unit: AirConcentrationUnit::UgPerM3,
            q_air: 0.1,
            q_unit: AirFlowUnit::M3PerS,
            horizon_s: 3600.0,
            ecoimpactscore: 0.9,
            hazard_weight: 2.0,
            karma_per_unit: 1.0e6,
            notes: "Test node".to_string(),
        };

        let cout = 15.0;
        let impact = evaluate_cyboair_impact(&meta, cout);

        assert!(impact.mass_removed > 0.0);
        assert!(impact.canonical_impact > 0.0);
        assert!(impact.nano_karma_bytes > 0.0);
    }

    #[test]
    fn test_system_impact() {
        let meta = CyboAirNodeMeta {
            node_id: AirNodeId("PHX-ARTERIAL-01".to_string()),
            label: "Phoenix arterial canopy".to_string(),
            pollutant: AirPollutant::Pm25,
            cin_baseline: 25.0,
            cin_unit: AirConcentrationUnit::UgPerM3,
            cref: 10.0,
            cref_unit: AirConcentrationUnit::UgPerM3,
            q_air: 0.1,
            q_unit: AirFlowUnit::M3PerS,
            horizon_s: 3600.0,
            ecoimpactscore: 0.9,
            hazard_weight: 2.0,
            karma_per_unit: 1.0e6,
            notes: "Test node".to_string(),
        };

        let nodes = vec![meta.clone(), meta];
        let couts = vec![15.0, 18.0];

        let (m, k_can, k_bytes) = evaluate_cyboair_system_impact(&nodes, &couts);
        assert!(m > 0.0);
        assert!(k_can > 0.0);
        assert!(k_bytes > 0.0);
    }
}
