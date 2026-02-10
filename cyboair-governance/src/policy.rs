#![forbid(unsafe_code)]

use crate::types::*;
use async_trait::async_trait;
use gatehouse::{AccessDecision, AccessEvaluation, PermissionChecker, Policy, PolicyEvalResult};

/// RBAC: static role -> coarse permissions.
pub struct RbacPolicy;

#[async_trait]
impl Policy<User, Resource, Action, EnvironmentCtx> for RbacPolicy {
    async fn evaluate_access(
        &self,
        user: &User,
        _res: &Resource,
        action: &Action,
        _env: &EnvironmentCtx,
    ) -> PolicyEvalResult {
        use Action::*;
        use Role::*;

        let allowed = match (&user.role, action) {
            (Superchair, _) => true,
            (Stakeholder, Read) | (Stakeholder, Write) => true,
            (Stakeholder, ExecuteControlProposal) | (Stakeholder, Export) => false,
            (Staff, Read) | (Staff, Write) | (Staff, ExecuteControlProposal) => true,
            (Staff, Export) => false,
            (Guest, Read) => true,
            (Guest, Write) | (Guest, ExecuteControlProposal) | (Guest, Export) => false,
            (Bot, Read) | (Bot, Write) => true,
            (Bot, ExecuteControlProposal) | (Bot, Export) => false,
        };

        if allowed {
            PolicyEvalResult::granted("RbacPolicy", Some("role grants action".into()))
        } else {
            PolicyEvalResult::denied("RbacPolicy", "role does not grant action")
        }
    }

    fn policy_type(&self) -> String {
        "RbacPolicy".into()
    }
}

/// ABAC: attributes (city, owner_id, department, time, TLS, etc.).
pub struct AbacPolicy;

#[async_trait]
impl Policy<User, Resource, Action, EnvironmentCtx> for AbacPolicy {
    async fn evaluate_access(
        &self,
        user: &User,
        res: &Resource,
        action: &Action,
        env: &EnvironmentCtx,
    ) -> PolicyEvalResult {
        use Action::*;
        use PropertyValue::*;

        // Example: stakeholder can only access resources they own.
        if matches!(user.role, Role::Stakeholder) {
            if let Some(owner_prop) = res.properties.get("owner_id") {
                if let Str(owner_id) = owner_prop {
                    if owner_id != &user.user_id {
                        return PolicyEvalResult::denied(
                            "AbacPolicy",
                            "stakeholder not owner of resource",
                        );
                    }
                }
            } else {
                return PolicyEvalResult::denied(
                    "AbacPolicy",
                    "resource missing owner_id for stakeholder",
                );
            }
        }

        // Example: staff must be PhoenixOps for Phoenix nodes.
        if matches!(user.role, Role::Staff) && matches!(action, ExecuteControlProposal) {
            if let Some(PropertyValue::Str(city)) = res.properties.get("city") {
                if city == "Phoenix" {
                    let dept_ok = matches!(
                        user.attributes.get("department"),
                        Some(AttributeValue::Str(d)) if d == "PhoenixOps"
                    );
                    if !dept_ok {
                        return PolicyEvalResult::denied(
                            "AbacPolicy",
                            "staff not in PhoenixOps for Phoenix node",
                        );
                    }
                }
            }
        }

        // Example: enforce encrypted channel for any write or execute.
        if matches!(action, Write | ExecuteControlProposal) && !env.is_encrypted_channel {
            return PolicyEvalResult::denied(
                "AbacPolicy",
                "unencrypted channel not allowed for privileged actions",
            );
        }

        // Additional time-based or IP-based conditions can be plugged here.

        PolicyEvalResult::granted("AbacPolicy", Some("ABAC conditions satisfied".into()))
    }

    fn policy_type(&self) -> String {
        "AbacPolicy".into()
    }
}

/// Central governance core: this is your F_policy implementation.
pub struct GovernanceCore {
    checker: PermissionChecker<User, Resource, Action, EnvironmentCtx>,
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
        user: &User,
        res: &Resource,
        action: &Action,
        env: &EnvironmentCtx,
    ) -> AccessEvaluation {
        self.checker.evaluate_access(user, res, action, env).await
    }
}
