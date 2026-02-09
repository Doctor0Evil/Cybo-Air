#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt::Debug;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Metric families across bee, marine, and urban (UHI) domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricFamily {
    BeeThermal,
    BeeChem,
    BeeEMF,
    BeeNoise,
    MarineThermal,
    MarineSalinity,
    MarineShear,
    MarineNoise,
    UrbanHeatIndex,
    UrbanWBGT,
    UrbanNOx,
    // Extend but never relax existing bee/marine bands.
}

/// Invariants are math objects, not ad-hoc checks.
pub trait CorridorInvariant<T>: Clone + Debug + PartialEq {
    /// Returns true iff all corridor constraints hold for this sample.
    fn holds(&self, sample: &T) -> bool;

    /// Non‑negative residual (e.g., Lyapunov potential); 0 is ideal.
    fn residual(&self, sample: &T) -> f64;
}

/// Anything that can be serialized into the corridor spine.
pub trait EcoBandCapable: for<'a> Deserialize<'a> + Serialize {
    /// Metric family this band applies to.
    fn metric_family(&self) -> MetricFamily;
}

/// Host budget (load allocation) plus eco-band and DW ceiling semantics.
pub trait HostBudgetEnvelope: Sized {
    type Band: EcoBandCapable;
    type Invariant: CorridorInvariant<Self>;

    /// Normalized host‑budget index in [0,1]; 1 is corridor edge.
    fn host_budget_index(&self) -> f64;

    /// Normalized eco‑band index in [0,1]; 1 is band edge.
    fn eco_band_index(&self) -> f64;

    /// Normalized degradation‑weighted ceiling index in [0,1].
    fn dw_ceiling_index(&self) -> f64;

    /// Hard safety gate (used by CI and runtime).
    fn is_within_envelope(&self, inv: &Self::Invariant) -> bool {
        self.host_budget_index() <= 1.0
            && self.eco_band_index() <= 1.0
            && self.dw_ceiling_index() <= 1.0
            && inv.holds(self)
    }
}

/// Any state that lives inside a safety envelope.
pub trait SafetyEnvelopeState: Clone {
    type Envelope: HostBudgetEnvelope;

    /// Underlying envelope snapshot for this state.
    fn envelope(&self) -> &Self::Envelope;
}

/// Domain invariants that must hold for hysteresis to be legal.
pub trait DomainInvariant: Clone {
    /// Compile‑time friendly inequality such as host_budget ≤ 0.85 * eco_band.
    fn host_vs_eco_ok(&self) -> bool;
}

/// Hysteresis logic tied directly to a safety envelope and its invariant.
pub trait HysteresisRule<S>
where
    S: SafetyEnvelopeState,
{
    type Inv: CorridorInvariant<S::Envelope> + DomainInvariant;

    /// Compute next state given a proposed input; may clamp for safety.
    fn next_state(&self, current: &S, proposed: &S, inv: &Self::Inv) -> S;
}

/// Example triggers across domains.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscalationTrigger {
    BeeColonyStress,
    BeeThermalDrift,
    BeeEMFOverload,
    MarinePHDrift,
    MarineLarvaeShearRisk,
    MarineNoiseStress,
    UrbanUHIOverheat,
    UrbanNightWBGTDrift,
    UrbanNOxSpike,
}

/// Abstract escalation action, to be bound by higher layers
/// (routing, governance, throttle policies).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscalationAction {
    ThrottleDutyCycle,
    DisableActuation,
    ReroutePath,
    EnterSensingOnly,
    TriggerAudit,
    TriggerAlert,
}

/// Escalation policy as supertrait: decides when to trigger domain actions.
pub trait EscalationPolicy<S>: HysteresisRule<S>
where
    S: SafetyEnvelopeState,
{
    /// Domain‑specific escalation trigger classification.
    fn classify_trigger(&self, state: &S) -> Option<EscalationTrigger>;

    /// Map trigger to external, corridor‑safe actions.
    fn escalation_actions(&self, trig: EscalationTrigger) -> Vec<EscalationAction>;
}

/// Degradation‑weighted ceiling per metric family.
///
/// Sealed: only crate‑local implementations so external code
/// cannot relax ceilings for bees or marine larvae.
pub trait CorridorCeiling: private::Sealed {
    const FAMILY: MetricFamily;
    /// Minimal allowed DW ceiling (e.g. 2.1 °C for Apis mellifera deployment).
    const DW_CEILING_MIN: f64;
    /// Hard maximal value; anything above is a CI error.
    const DW_CEILING_MAX: f64;
}

mod private {
    pub trait Sealed {}
}

/// Bee thermal ceiling (brood/hive corridors).
pub struct BeeThermalCeiling;
impl private::Sealed for BeeThermalCeiling {}
impl CorridorCeiling for BeeThermalCeiling {
    const FAMILY: MetricFamily = MetricFamily::BeeThermal;
    // These are conservative placeholders: real values should come
    // from signed BeeNeuralCorridorPhoenix*.aln tables.
    const DW_CEILING_MIN: f64 = 2.1;
    const DW_CEILING_MAX: f64 = 3.0;
}

/// Marine larvae thermal ceiling (tighter than bee).
pub struct MarineLarvaeThermalCeiling;
impl private::Sealed for MarineLarvaeThermalCeiling {}
impl CorridorCeiling for MarineLarvaeThermalCeiling {
    const FAMILY: MetricFamily = MetricFamily::MarineThermal;
    const DW_CEILING_MIN: f64 = 0.5;
    const DW_CEILING_MAX: f64 = 1.5;
}

/// Generic host‑budget band in integer hundredths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostBudgetBand {
    /// Minimum budget (0–100, interpreted as percentage).
    pub min: u8,
    /// Maximum budget (0–100).
    pub max: u8,
}

/// Corridor band tying a host‑budget band to a specific ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorridorBand<C: CorridorCeiling> {
    pub budget: HostBudgetBand,
    pub _ceiling: core::marker::PhantomData<C>,
}

impl<C: CorridorCeiling> CorridorBand<C> {
    /// CI‑oriented check: host budget upper bound cannot exceed
    /// 0.85 × DW_CEILING_MIN (scaled into the same 0–100 space).
    pub const fn budget_within_ceiling(&self) -> bool {
        let ceiling_scaled = (C::DW_CEILING_MIN * 10.0) as u8; // map °C into [0,255]
        // 85% bound.
        let allowed = ((ceiling_scaled as u16) * 85 / 100) as u8;
        self.budget.max <= allowed
    }
}

/// Traceability: corridor IDs and wire format.
pub trait Traceable {
    fn corridor_trace_id(&self) -> Uuid;
}

pub trait BinaryEcoTrace: Traceable {
    /// Serialize into Corridor‑Research Spine wire format.
    fn to_wire_bytes(&self) -> Vec<u8>;
}

/* =========================
   Concrete bee envelope
   ========================= */

/// Bee corridor band with normalized indices.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeeBand {
    pub family: MetricFamily,
    /// Normalized host‑budget index [0,1].
    pub host_budget: f64,
    /// Normalized eco‑band index [0,1].
    pub eco_band: f64,
    /// Normalized DW ceiling index [0,1].
    pub dw_ceiling: f64,
}

impl EcoBandCapable for BeeBand {
    fn metric_family(&self) -> MetricFamily {
        self.family
    }
}

/// Minimal bee envelope struct, compatible with Bee Safety Kernel semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeeEnvelope {
    pub band: BeeBand,
    pub trace_id: Uuid,
}

impl HostBudgetEnvelope for BeeEnvelope {
    type Band = BeeBand;
    type Invariant = BeeCorridorInvariant;

    fn host_budget_index(&self) -> f64 {
        self.band.host_budget
    }

    fn eco_band_index(&self) -> f64 {
        self.band.eco_band
    }

    fn dw_ceiling_index(&self) -> f64 {
        self.band.dw_ceiling
    }
}

/// Bee Lyapunov‑style invariant; residual approximates Vbee.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeeCorridorInvariant {
    /// Safe residual threshold (e.g. Vsafe from your BeeRiskWeights).
    pub v_safe: f64,
}

impl CorridorInvariant<BeeEnvelope> for BeeCorridorInvariant {
    fn holds(&self, sample: &BeeEnvelope) -> bool {
        self.residual(sample) <= self.v_safe
    }

    fn residual(&self, sample: &BeeEnvelope) -> f64 {
        // Simple quadratic residual over normalized indices; this mirrors
        // your Vbee = Σ w_x r_x^2 structure in a minimal form.
        let hb = sample.band.host_budget.max(0.0);
        let eco = sample.band.eco_band.max(0.0);
        let dw = sample.band.dw_ceiling.max(0.0);
        hb * hb + eco * eco + dw * dw
    }
}

/// Bee state inside an envelope; can be extended with TDI, MBI, etc.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeeState {
    pub envelope: BeeEnvelope,
    /// Example: aggregate BeeHBScore in [0,1].
    pub hb_score: f64,
}

impl SafetyEnvelopeState for BeeState {
    type Envelope = BeeEnvelope;

    fn envelope(&self) -> &Self::Envelope {
        &self.envelope
    }
}

/// Domain invariant for bees: host_budget must always be ≤ 0.85 * eco_band.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeeDomainInvariant;

impl DomainInvariant for BeeDomainInvariant {
    fn host_vs_eco_ok(&self) -> bool {
        // The actual inequality is evaluated in the hysteresis rule,
        // this value is a compile‑time / config sanity hook.
        true
    }
}

/// Simple bee hysteresis: clamp proposed state if envelope or inequality fail.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeeHysteresisRule;

impl HysteresisRule<BeeState> for BeeHysteresisRule {
    type Inv = BeeCorridorInvariant;

    fn next_state(
        &self,
        current: &BeeState,
        proposed: &BeeState,
        inv: &Self::Inv,
    ) -> BeeState {
        let env = &proposed.envelope;
        let hb = env.band.host_budget;
        let eco = env.band.eco_band;

        // Enforce host_budget ≤ 0.85 * eco_band at run time.
        if eco <= 0.0 {
            return current.clone();
        }
        if hb > 0.85 * eco {
            return current.clone();
        }
        if !env.is_within_envelope(inv) {
            return current.clone();
        }
        proposed.clone()
    }
}

/// Escalation policy for bee states.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeeEscalationPolicy;

impl HysteresisRule<BeeState> for BeeEscalationPolicy {
    type Inv = BeeCorridorInvariant;

    fn next_state(
        &self,
        current: &BeeState,
        proposed: &BeeState,
        inv: &Self::Inv,
    ) -> BeeState {
        BeeHysteresisRule.next_state(current, proposed, inv)
    }
}

impl EscalationPolicy<BeeState> for BeeEscalationPolicy {
    fn classify_trigger(&self, state: &BeeState) -> Option<EscalationTrigger> {
        let env = &state.envelope;
        let hb = env.band.host_budget;
        let eco = env.band.eco_band;

        if hb > 0.9 {
            Some(EscalationTrigger::BeeColonyStress)
        } else if hb > 0.85 * eco {
            Some(EscalationTrigger::BeeThermalDrift)
        } else {
            None
        }
    }

    fn escalation_actions(&self, trig: EscalationTrigger) -> Vec<EscalationAction> {
        match trig {
            EscalationTrigger::BeeColonyStress => vec![
                EscalationAction::DisableActuation,
                EscalationAction::EnterSensingOnly,
                EscalationAction::TriggerAudit,
            ],
            EscalationTrigger::BeeThermalDrift => vec![
                EscalationAction::ThrottleDutyCycle,
                EscalationAction::ReroutePath,
                EscalationAction::TriggerAlert,
            ],
            _ => vec![EscalationAction::TriggerAudit],
        }
    }
}

impl Traceable for BeeEnvelope {
    fn corridor_trace_id(&self) -> Uuid {
        self.trace_id
    }
}

impl BinaryEcoTrace for BeeEnvelope {
    fn to_wire_bytes(&self) -> Vec<u8> {
        // Use postcard or bincode; postcard fits no_std better.
        postcard::to_allocvec(self).unwrap_or_default()
    }
}

/* =========================
   Unit tests (std only)
   ========================= */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bee_band_metric_family() {
        let band = BeeBand {
            family: MetricFamily::BeeThermal,
            host_budget: 0.4,
            eco_band: 0.6,
            dw_ceiling: 0.3,
        };
        assert_eq!(band.metric_family(), MetricFamily::BeeThermal);
    }

    #[test]
    fn bee_envelope_invariant_residual() {
        let band = BeeBand {
            family: MetricFamily::BeeThermal,
            host_budget: 0.4,
            eco_band: 0.6,
            dw_ceiling: 0.3,
        };
        let env = BeeEnvelope {
            band,
            trace_id: Uuid::nil(),
        };
        let inv = BeeCorridorInvariant { v_safe: 1.0 };
        let res = inv.residual(&env);
        assert!(res >= 0.0);
        assert!(inv.holds(&env));
    }

    #[test]
    fn bee_hysteresis_clamps_unsafe_state() {
        let band_current = BeeBand {
            family: MetricFamily::BeeThermal,
            host_budget: 0.4,
            eco_band: 0.6,
            dw_ceiling: 0.3,
        };
        let env_current = BeeEnvelope {
            band: band_current,
            trace_id: Uuid::nil(),
        };
        let state_current = BeeState {
            envelope: env_current,
            hb_score: 0.98,
        };

        let band_proposed = BeeBand {
            family: MetricFamily::BeeThermal,
            host_budget: 0.95, // exceeds 0.85 * eco_band
            eco_band: 0.6,
            dw_ceiling: 0.4,
        };
        let env_proposed = BeeEnvelope {
            band: band_proposed,
            trace_id: Uuid::nil(),
        };
        let state_proposed = BeeState {
            envelope: env_proposed,
            hb_score: 0.9,
        };

        let inv = BeeCorridorInvariant { v_safe: 10.0 };
        let rule = BeeHysteresisRule;

        let next = rule.next_state(&state_current, &state_proposed, &inv);
        assert!((next.envelope.band.host_budget - 0.4).abs() < 1e-6);
    }

    #[test]
    fn corridor_band_respects_ceiling() {
        let band = HostBudgetBand { min: 0, max: 15 };
        let corridor: CorridorBand<BeeThermalCeiling> = CorridorBand {
            budget: band,
            _ceiling: core::marker::PhantomData,
        };
        assert!(corridor.budget_within_ceiling());
    }
}
