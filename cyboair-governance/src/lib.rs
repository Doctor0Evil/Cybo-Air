#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use gatehouse::{
    AccessEvaluation, PermissionChecker, Policy, PolicyEvalResult,
};
use async_trait::async_trait;

// ---- Domain core types ----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Superchair,
    Stakeholder,
    Staff,
    Guest,
    Bot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    ReadShard,
    WriteTelemetry,
    ProposeControl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub role: Role,
    /// Arbitrary attributes for ABAC (e.g. "owns_node=node_01").
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub resource_id: String,
    /// Owner DID or stakeholder id for ABAC checks.
    pub owner: Option<String>,
    /// Node- / shard-level attributes: ecobranch, zone, etc.
    pub attributes: Vec<(String, String)>,
}

// Simple context wrapper if you need extra metadata (tenant, time, etc.).
#[derive(Debug, Clone, Default)]
pub struct GovContext;

// ---- Gatehouse policies: RBAC + ABAC composition -------------------------

/// RBAC: map Role + Action to a coarse allow/deny.
pub struct RbacPolicy;

#[async_trait]
impl Policy<Principal, Resource, Action, GovContext> for RbacPolicy {
    async fn evaluate_access(
        &self,
        principal: &Principal,
        action: &Action,
        _resource: &Resource,
        _ctx: &GovContext,
    ) -> PolicyEvalResult {
        use Action::*;
        use Role::*;

        let allowed = match (&principal.role, action) {
            (Superchair, _) => true,
            (Stakeholder, ReadShard) => true,
            (Stakeholder, WriteTelemetry) => true,
            (Stakeholder, ProposeControl) => false,
            (Staff, ReadShard) | (Staff, WriteTelemetry) | (Staff, ProposeControl) => true,
            (Guest, ReadShard) => true, // Public-only enforced in ABAC.
            (Guest, WriteTelemetry) | (Guest, ProposeControl) => false,
            (Bot, ReadShard) | (Bot, WriteTelemetry) => true,
            (Bot, ProposeControl) => false,
        };

        if allowed {
            PolicyEvalResult::granted("RbacPolicy", Some("role grants action".into()))
        } else {
            PolicyEvalResult::denied("RbacPolicy", "role does not grant action")
        }
    }

    fn policy_type(&self) -> String {
        "RbacPolicy".to_string()
    }
}

/// ABAC: stakeholders may only touch their own nodes, guests only public, etc.
pub struct AbacPolicy;

#[async_trait]
impl Policy<Principal, Resource, Action, GovContext> for AbacPolicy {
    async fn evaluate_access(
        &self,
        principal: &Principal,
        action: &Action,
        resource: &Resource,
        _ctx: &GovContext,
    ) -> PolicyEvalResult {
        use Action::*;
        use Role::*;

        // Owner-based constraint for stakeholders.
        if matches!(principal.role, Stakeholder) {
            if let Some(owner) = &resource.owner {
                if owner != &principal.id {
                    return PolicyEvalResult::denied(
                        "AbacPolicy",
                        "stakeholder not owner of resource",
                    );
                }
            } else {
                return PolicyEvalResult::denied(
                    "AbacPolicy",
                    "resource has no owner metadata",
                );
            }
        }

        // Guest can only read public shards: enforce via resource attributes.
        if matches!(principal.role, Guest) {
            if let ReadShard = action {
                let is_public = resource
                    .attributes
                    .iter()
                    .any(|(k, v)| k == "visibility" && v == "public");
                if !is_public {
                    return PolicyEvalResult::denied(
                        "AbacPolicy",
                        "guest cannot read non-public resource",
                    );
                }
            }
        }

        PolicyEvalResult::granted("AbacPolicy", Some("ABAC conditions satisfied".into()))
    }

    fn policy_type(&self) -> String {
        "AbacPolicy".to_string()
    }
}

// ---- GovernanceCore: single entry point for callers -----------------------

pub struct GovernanceCore {
    checker: PermissionChecker<Principal, Resource, Action, GovContext>,
}

impl GovernanceCore {
    pub fn new() -> Self {
        let mut checker = PermissionChecker::new();
        checker.add_policy(RbacPolicy);
        checker.add_policy(AbacPolicy);
        Self { checker }
    }

    pub async fn authorize(
        &self,
        principal: &Principal,
        action: &Action,
        resource: &Resource,
        ctx: &GovContext,
    ) -> AccessEvaluation {
        self.checker
            .evaluate_access(principal, action, resource, ctx)
            .await
    }
}

// ---- Input guards --------------------------------------------------------

pub struct InputGuard;

impl InputGuard {
    pub fn validate_duty_cycle(duty_cycle: f64) -> Result<(), String> {
        if (0.0..=1.0).contains(&duty_cycle) {
            Ok(())
        } else {
            Err("duty_cycle must be between 0.0 and 1.0".to_string())
        }
    }

    // Here you would add:
    // - schema validation for proposals,
    // - CEIM / RoH range checks,
    // - bee corridor envelope checks, etc.
}

// ---- Generator–verifier pipeline types -----------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct Proposal {
    pub node_ids: Vec<String>,
    pub duty_cycles: Vec<f64>,
    // plus CEIM, NanoKarma, Beekarma deltas, horizon, etc.
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub approved: bool,
    pub message: String,
}

pub struct Generator;

impl Generator {
    /// Purely constructs a proposal; does not apply it.
    pub fn generate_proposal(_task: &str) -> Proposal {
        // In production, this would call your LLM/heuristic and
        // enforce output schema.
        Proposal {
            node_ids: Vec::new(),
            duty_cycles: Vec::new(),
        }
    }
}

pub struct Verifier;

impl Verifier {
    /// Core safety and governance checks; only source of "approved".
    pub fn verify(proposal: &Proposal) -> Verdict {
        // 1. Size and basic sanity constraints.
        if proposal.node_ids.len() != proposal.duty_cycles.len() {
            return Verdict {
                approved: false,
                message: "node_ids and duty_cycles length mismatch".into(),
            };
        }

        // 2. Local numeric checks.
        for dc in &proposal.duty_cycles {
            if let Err(e) = InputGuard::validate_duty_cycle(*dc) {
                return Verdict {
                    approved: false,
                    message: format!("invalid duty_cycle: {e}"),
                };
            }
        }

        // 3. TODO: integrate CEIM, RoH, NanoKarma, Beekarma, BeeSafetyKernel:
        //    - project proposal into qpudatashards and CEIM corridors,
        //    - enforce RoH_after <= RoH_before <= 0.3,
        //    - enforce BeeNeuralSafe & BeeHBScore invariants,
        //    - enforce TECHPolicyDocument budgets.

        Verdict {
            approved: true,
            message: "proposal passed core governance checks".into(),
        }
    }
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gatehouse::AccessDecision;

    fn mk_core() -> GovernanceCore {
        GovernanceCore::new()
    }

    #[tokio::test]
    async fn test_authorization_schema() {
        let core = mk_core();

        let superchair = Principal {
            id: "admin@cyboair.org".into(),
            role: Role::Superchair,
            attributes: vec![],
        };
        let stakeholder = Principal {
            id: "sh@org.com".into(),
            role: Role::Stakeholder,
            attributes: vec![],
        };

        let resource = Resource {
            resource_id: "node_01".into(),
            owner: Some("sh@org.com".into()),
            attributes: vec![("visibility".into(), "restricted".into())],
        };

        // Superchair: allowed to propose control.
        let eval = core
            .authorize(&superchair, &Action::ProposeControl, &resource, &GovContext)
            .await;
        assert!(matches!(eval.decision, AccessDecision::Granted));

        // Stakeholder: not allowed to propose control.
        let eval = core
            .authorize(&stakeholder, &Action::ProposeControl, &resource, &GovContext)
            .await;
        assert!(matches!(eval.decision, AccessDecision::Denied));
    }

    #[test]
    fn test_input_guard_duty_cycle() {
        assert!(InputGuard::validate_duty_cycle(0.0).is_ok());
        assert!(InputGuard::validate_duty_cycle(0.5).is_ok());
        assert!(InputGuard::validate_duty_cycle(1.0).is_ok());
        assert!(InputGuard::validate_duty_cycle(-0.1).is_err());
        assert!(InputGuard::validate_duty_cycle(1.1).is_err());
    }
}
