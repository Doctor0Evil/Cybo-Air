use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Superchair,
    Stakeholder,
    Staff,
    Guest,
    Bot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceType {
    Shard,
    Node,
    TelemetryStream,
    ControlProposal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributeValue {
    Str(String),
    Bool(bool),
    Int(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    Str(String),
    Bool(bool),
    Int(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_id: String,
    pub role: Role,
    pub attributes: HashMap<String, AttributeValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub resource_id: String,
    pub resource_type: ResourceType,
    pub properties: HashMap<String, PropertyValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    ExecuteControlProposal,
    Export,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentCtx {
    pub time_utc: chrono::DateTime<chrono::Utc>,
    pub ip_address: String,
    pub is_encrypted_channel: bool,
}
