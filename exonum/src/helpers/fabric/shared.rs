//! This module is used, to collect structures, 
//! that is shared into `CommandExtension` from `Command`.
//!
use toml::Value;
use std::collections::BTreeMap;
use std::net::SocketAddr;

use crypto::{PublicKey, SecretKey};
use blockchain::config::ConsensusConfig;
use blockchain::config::ValidatorKeys;

pub type AbstractConfig = BTreeMap<String, Value>;

/// `NodePublicConfig` contain public node config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePublicConfig {
    pub addr: SocketAddr,
    pub validator_keys: ValidatorKeys,
    pub services_public_configs: AbstractConfig
}

/// `SharedConfig` contain all public information
/// that should be shared in handshake process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedConfig {
    pub common: CommonConfigTemplate,
    pub node: NodePublicConfig,
}

impl NodePublicConfig {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn services_public_configs(&self) -> &AbstractConfig {
        &self.services_public_configs
    }
}

/// Basepoint config.
#[derive(PartialEq, Clone, Debug, Serialize, Deserialize, Default)]
pub struct CommonConfigTemplate {
    pub consensus_config: ConsensusConfig,
    pub services_config: AbstractConfig,
}

/// `NodePrivConfig` collect all public and secret keys.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodePrivateConfig {
    /// Listen addr.
    pub listen_addr: SocketAddr,
    /// Consensus public key.
    pub consensus_public_key: PublicKey,
    /// Consensus secret key.
    pub consensus_secret_key: SecretKey,
    /// Service public key.
    pub service_public_key: PublicKey,
    /// Service secret key.
    pub service_secret_key: SecretKey,
    /// Additional service secret config.
    pub services_secret_configs: AbstractConfig
}
