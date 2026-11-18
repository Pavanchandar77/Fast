use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::error::ShieldError;

pub const FWPM_SESSION_FLAG_DYNAMIC: u32 = 0x00000001;
pub const FWPM_SESSION_FLAG_PERSISTENT: u32 = 0x00000002;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WfpDirection {
    Inbound,
    Outbound,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WfpAction {
    Block,
    Permit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WfpProtocol {
    Any,
    Tcp,
    Udp,
    Icmp,
}

impl WfpProtocol {
    pub fn ip_protocol_number(&self) -> Option<u8> {
        match self {
            Self::Any => None,
            Self::Icmp => Some(1),
            Self::Tcp => Some(6),
            Self::Udp => Some(17),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WfpCondition {
    AppPath(String),
    RemoteIp(IpAddr),
    RemotePort(u16),
    LocalPort(u16),
    Protocol(WfpProtocol),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WfpRule {
    pub rule_id: u64,
    pub name: String,
    pub description: String,
    pub action: WfpAction,
    pub direction: WfpDirection,
    pub conditions: Vec<WfpCondition>,
    pub weight: u64,
    pub persistent: bool,
}

impl WfpRule {
    pub fn builder(name: impl Into<String>) -> WfpRuleBuilder {
        WfpRuleBuilder::new(name)
    }

    pub fn app_path(&self) -> Option<&str> {
        self.conditions.iter().find_map(|c| match c {
            WfpCondition::AppPath(p) => Some(p.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct WfpRuleBuilder {
    name: String,
    description: String,
    action: WfpAction,
    direction: WfpDirection,
    conditions: Vec<WfpCondition>,
    weight: u64,
    persistent: bool,
}

impl WfpRuleBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            action: WfpAction::Block,
            direction: WfpDirection::Outbound,
            conditions: Vec::new(),
            weight: 100,
            persistent: false, // Default is dynamic (non-persistent)
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn for_app(mut self, path: impl Into<String>) -> Self {
        self.conditions.push(WfpCondition::AppPath(path.into()));
        self
    }

    pub fn action(mut self, action: WfpAction) -> Self {
        self.action = action;
        self
    }

    pub fn direction(mut self, direction: WfpDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn block_outbound(mut self) -> Self {
        self.action = WfpAction::Block;
        self.direction = WfpDirection::Outbound;
        self
    }

    pub fn block_inbound(mut self) -> Self {
        self.action = WfpAction::Block;
        self.direction = WfpDirection::Inbound;
        self
    }

    pub fn permit_outbound(mut self) -> Self {
        self.action = WfpAction::Permit;
        self.direction = WfpDirection::Outbound;
        self
    }

    pub fn permit_inbound(mut self) -> Self {
        self.action = WfpAction::Permit;
        self.direction = WfpDirection::Inbound;
        self
    }

    pub fn with_remote_ip(mut self, ip: IpAddr) -> Self {
        self.conditions.push(WfpCondition::RemoteIp(ip));
        self
    }

    pub fn with_remote_port(mut self, port: u16) -> Self {
        self.conditions.push(WfpCondition::RemotePort(port));
        self
    }

    pub fn with_local_port(mut self, port: u16) -> Self {
        self.conditions.push(WfpCondition::LocalPort(port));
        self
    }

    pub fn with_protocol(mut self, protocol: WfpProtocol) -> Self {
        self.conditions.push(WfpCondition::Protocol(protocol));
        self
    }

    pub fn with_condition(mut self, condition: WfpCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    pub fn weight(mut self, weight: u64) -> Self {
        self.weight = weight;
        self
    }

    /// Persistent rules stay in the Windows BFE database across reboots.
    /// By default this is false (session is dynamic, flushed when Nexus dies).
    pub fn persistent(mut self, persistent: bool) -> Self {
        self.persistent = persistent;
        self
    }

    pub fn build(self) -> Result<WfpRule, ShieldError> {
        if self.name.trim().is_empty() {
            return Err(ShieldError::WfpValidation(
                "Rule name cannot be empty".into(),
            ));
        }
        if self.conditions.is_empty() {
            return Err(ShieldError::WfpValidation(
                "Rule must have at least one matching condition".into(),
            ));
        }

        static RULE_ID_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1000);
        let rule_id = RULE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(WfpRule {
            rule_id,
            name: self.name,
            description: self.description,
            action: self.action,
            direction: self.direction,
            conditions: self.conditions,
            weight: self.weight,
            persistent: self.persistent,
        })
    }
}

/// Generated struct matching Windows WFP filter descriptor for inspection/testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WfpFilterSpec {
    pub filter_key_name: String,
    pub layer_name: String,
    pub sublayer_key: String,
    pub action_type: String,
    pub weight: u64,
    pub condition_count: usize,
    pub flags: u32,
    pub condition_descriptors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WfpSessionConfig {
    pub name: String,
    pub description: String,
    pub flags: u32,
    pub is_dynamic: bool,
}

impl Default for WfpSessionConfig {
    fn default() -> Self {
        Self {
            name: "Nexus-OS Shield Dynamic Session".into(),
            description: "Dynamic kernel filtering session, auto-flushed on process exit".into(),
            flags: FWPM_SESSION_FLAG_DYNAMIC,
            is_dynamic: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WfpEngine {
    pub session: WfpSessionConfig,
    pub rules: BTreeMap<u64, WfpRule>,
}

impl Default for WfpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WfpEngine {
    /// Constructs a WFP engine with a DYNAMIC session by default.
    /// This prevents permanent network loss if Nexus crashes or is terminated.
    pub fn new() -> Self {
        Self {
            session: WfpSessionConfig::default(),
            rules: BTreeMap::new(),
        }
    }

    /// Constructs a WFP engine with persistent session configuration (user opt-in).
    pub fn new_persistent() -> Self {
        Self {
            session: WfpSessionConfig {
                name: "Nexus-OS Shield Persistent Session".into(),
                description: "Persistent kernel filtering session surviving system restarts".into(),
                flags: FWPM_SESSION_FLAG_PERSISTENT,
                is_dynamic: false,
            },
            rules: BTreeMap::new(),
        }
    }

    pub fn is_dynamic(&self) -> bool {
        self.session.is_dynamic
    }

    pub fn add_rule(&mut self, rule: WfpRule) -> u64 {
        let id = rule.rule_id;
        self.rules.insert(id, rule);
        id
    }

    pub fn remove_rule(&mut self, rule_id: u64) -> Result<WfpRule, ShieldError> {
        self.rules.remove(&rule_id).ok_or_else(|| {
            ShieldError::WfpValidation(format!("Rule ID #{rule_id} not found in WFP engine"))
        })
    }

    pub fn get_rule(&self, rule_id: u64) -> Option<&WfpRule> {
        self.rules.get(&rule_id)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Generates testable Win32 WFP filter specifications from active rules.
    pub fn generate_filter_specs(&self) -> Vec<WfpFilterSpec> {
        self.rules
            .values()
            .map(|r| {
                let layer_name = match r.direction {
                    WfpDirection::Outbound => "FWPM_LAYER_ALE_AUTH_CONNECT_V4",
                    WfpDirection::Inbound => "FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4",
                    WfpDirection::Both => "FWPM_LAYER_ALE_FLOW_ESTABLISHED_V4",
                };
                let action_type = match r.action {
                    WfpAction::Block => "FWP_ACTION_BLOCK",
                    WfpAction::Permit => "FWP_ACTION_PERMIT",
                };
                let descriptors = r
                    .conditions
                    .iter()
                    .map(|c| match c {
                        WfpCondition::AppPath(p) => format!("FWPM_CONDITION_ALE_APP_ID == '{p}'"),
                        WfpCondition::RemoteIp(ip) => {
                            format!("FWPM_CONDITION_IP_REMOTE_ADDRESS == {ip}")
                        }
                        WfpCondition::RemotePort(port) => {
                            format!("FWPM_CONDITION_IP_REMOTE_PORT == {port}")
                        }
                        WfpCondition::LocalPort(port) => {
                            format!("FWPM_CONDITION_IP_LOCAL_PORT == {port}")
                        }
                        WfpCondition::Protocol(proto) => {
                            format!("FWPM_CONDITION_IP_PROTOCOL == {proto:?}")
                        }
                    })
                    .collect();

                let flags = if r.persistent {
                    FWPM_SESSION_FLAG_PERSISTENT
                } else {
                    FWPM_SESSION_FLAG_DYNAMIC
                };

                WfpFilterSpec {
                    filter_key_name: format!("nexus_filter_{}", r.rule_id),
                    layer_name: layer_name.into(),
                    sublayer_key: "nexus_sublayer_shield".into(),
                    action_type: action_type.into(),
                    weight: r.weight,
                    condition_count: r.conditions.len(),
                    flags,
                    condition_descriptors: descriptors,
                }
            })
            .collect()
    }
}
