//! Zcash network parameters.

use serde::{Deserialize, Serialize};
use zcash_protocol::{
    consensus::{self, BlockHeight},
    local_consensus,
};

/// Chain parameters for a Zcash consensus network.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Network {
    /// A public global consensus network.
    Consensus(consensus::Network),
    /// A Zebra configured Testnet that uses Testnet address and coin type
    /// constants with non-default consensus parameters.
    ConfiguredTestnet(ConfiguredTestnet),
    /// A local network used for integration testing.
    RegTest(local_consensus::LocalNetwork),
}

impl Network {
    pub(crate) fn from_config(
        network_kind: NetworkKind,
        nuparams: &[RegTestNuParam],
        configured_testnet: ConfiguredTestnet,
    ) -> Self {
        match network_kind {
            NetworkKind::Main => Self::Consensus(consensus::Network::MainNetwork),
            NetworkKind::Test => Self::Consensus(consensus::Network::TestNetwork),
            NetworkKind::ConfiguredTestnet => Self::ConfiguredTestnet(configured_testnet),
            NetworkKind::Regtest => {
                let find_nu = |nu: consensus::BranchId| {
                    nuparams
                        .iter()
                        .find(|p| p.consensus_branch_id == nu)
                        .map(|p| p.activation_height)
                };

                // If a NU is omitted, ensure that it activates at the same height as the
                // subsequent specified NU (if any).
                let nu6_1 = find_nu(consensus::BranchId::Nu6_1);
                let nu6 = find_nu(consensus::BranchId::Nu6).or(nu6_1);
                let nu5 = find_nu(consensus::BranchId::Nu5).or(nu6);
                let canopy = find_nu(consensus::BranchId::Canopy).or(nu5);
                let heartwood = find_nu(consensus::BranchId::Heartwood).or(canopy);
                let blossom = find_nu(consensus::BranchId::Blossom).or(heartwood);
                let sapling = find_nu(consensus::BranchId::Sapling).or(blossom);
                let overwinter = find_nu(consensus::BranchId::Overwinter).or(sapling);

                Self::RegTest(local_consensus::LocalNetwork {
                    overwinter,
                    sapling,
                    blossom,
                    heartwood,
                    canopy,
                    nu5,
                    nu6,
                    nu6_1,
                    #[cfg(zcash_unstable = "nu7")]
                    nu7: find_nu(consensus::BranchId::Nu7),
                    #[cfg(zcash_unstable = "zfuture")]
                    z_future: None,
                })
            }
        }
    }

    pub(crate) fn to_zaino(&self) -> zaino_common::Network {
        match self {
            Network::Consensus(network) => match network {
                consensus::Network::MainNetwork => zaino_common::Network::Mainnet,
                consensus::Network::TestNetwork => zaino_common::Network::Testnet,
            },
            Network::ConfiguredTestnet(params) => {
                zaino_common::Network::ConfiguredTestnet(zaino_common::ConfiguredTestnet {
                    network_name: params.network_name.clone(),
                    network_magic: params.network_magic,
                    genesis_hash: params.genesis_hash.clone(),
                    activation_heights: params.activation_heights.to_zaino(),
                })
            }
            // TODO: This does not create a compatible regtest network because Zebra does
            // not have the necessary flexibility.
            Network::RegTest(local_network) => {
                zaino_common::Network::Regtest(zaino_common::network::ActivationHeights {
                    before_overwinter: Some(1),
                    overwinter: local_network.overwinter.map(|h| h.into()),
                    sapling: local_network.sapling.map(|h| h.into()),
                    blossom: local_network.blossom.map(|h| h.into()),
                    heartwood: local_network.heartwood.map(|h| h.into()),
                    canopy: local_network.canopy.map(|h| h.into()),
                    nu5: local_network.nu5.map(|h| h.into()),
                    nu6: local_network.nu6.map(|h| h.into()),
                    nu6_1: local_network.nu6_1.map(|h| h.into()),
                    #[cfg(zcash_unstable = "nu7")]
                    nu7: local_network.nu7.map(|h| h.into()),
                    #[cfg(not(zcash_unstable = "nu7"))]
                    nu7: None,
                })
            }
        }
    }
}

impl consensus::Parameters for Network {
    fn network_type(&self) -> consensus::NetworkType {
        match self {
            Self::Consensus(params) => params.network_type(),
            Self::ConfiguredTestnet(params) => params.network_type(),
            Self::RegTest(params) => params.network_type(),
        }
    }

    fn activation_height(&self, nu: consensus::NetworkUpgrade) -> Option<BlockHeight> {
        match self {
            Self::Consensus(params) => params.activation_height(nu),
            Self::ConfiguredTestnet(params) => params.activation_height(nu),
            Self::RegTest(params) => params.activation_height(nu),
        }
    }
}

/// The supported network configuration modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NetworkKind {
    /// Mainnet.
    #[default]
    Main,
    /// Default public Testnet.
    Test,
    /// Zebra configured Testnet.
    ConfiguredTestnet,
    /// Regtest.
    Regtest,
}

impl NetworkKind {
    pub(crate) fn network_type(self) -> consensus::NetworkType {
        match self {
            Self::Main => consensus::NetworkType::Main,
            Self::Test | Self::ConfiguredTestnet => consensus::NetworkType::Test,
            Self::Regtest => consensus::NetworkType::Regtest,
        }
    }
}

/// Consensus parameters for Zebra configured Testnets.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ConfiguredTestnet {
    /// Configured Testnet name.
    pub network_name: String,
    /// Configured Testnet network magic bytes.
    pub network_magic: [u8; 4],
    /// Configured Testnet genesis block hash.
    pub genesis_hash: Option<String>,
    /// Configured Testnet activation heights.
    #[serde(default)]
    pub activation_heights: ConfiguredActivationHeights,
}

impl consensus::Parameters for ConfiguredTestnet {
    fn network_type(&self) -> consensus::NetworkType {
        consensus::NetworkType::Test
    }

    fn activation_height(&self, nu: consensus::NetworkUpgrade) -> Option<BlockHeight> {
        self.activation_heights.activation_height(nu)
    }
}

/// Configurable activation heights for Zebra configured Testnets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct ConfiguredActivationHeights {
    /// Activation height for `Overwinter`.
    pub overwinter: Option<u32>,
    /// Activation height for `Sapling`.
    pub sapling: Option<u32>,
    /// Activation height for `Blossom`.
    pub blossom: Option<u32>,
    /// Activation height for `Heartwood`.
    pub heartwood: Option<u32>,
    /// Activation height for `Canopy`.
    pub canopy: Option<u32>,
    /// Activation height for `NU5`.
    #[serde(rename = "NU5")]
    pub nu5: Option<u32>,
    /// Activation height for `NU6`.
    #[serde(rename = "NU6")]
    pub nu6: Option<u32>,
    /// Activation height for `NU6.1`.
    #[serde(rename = "NU6.1")]
    pub nu6_1: Option<u32>,
    /// Activation height for `NU7`.
    #[serde(rename = "NU7")]
    pub nu7: Option<u32>,
}

impl ConfiguredActivationHeights {
    fn activation_height(self, nu: consensus::NetworkUpgrade) -> Option<BlockHeight> {
        let height = match nu {
            consensus::NetworkUpgrade::Overwinter => self.overwinter,
            consensus::NetworkUpgrade::Sapling => self.sapling,
            consensus::NetworkUpgrade::Blossom => self.blossom,
            consensus::NetworkUpgrade::Heartwood => self.heartwood,
            consensus::NetworkUpgrade::Canopy => self.canopy,
            consensus::NetworkUpgrade::Nu5 => self.nu5,
            consensus::NetworkUpgrade::Nu6 => self.nu6,
            consensus::NetworkUpgrade::Nu6_1 => self.nu6_1,
            #[cfg(zcash_unstable = "nu7")]
            consensus::NetworkUpgrade::Nu7 => self.nu7,
            #[cfg(zcash_unstable = "zfuture")]
            consensus::NetworkUpgrade::ZFuture => None,
        };

        height.map(BlockHeight::from_u32)
    }

    fn to_zaino(self) -> zaino_common::network::ActivationHeights {
        zaino_common::network::ActivationHeights {
            before_overwinter: None,
            overwinter: self.overwinter,
            sapling: self.sapling,
            blossom: self.blossom,
            heartwood: self.heartwood,
            canopy: self.canopy,
            nu5: self.nu5,
            nu6: self.nu6,
            nu6_1: self.nu6_1,
            nu7: self.nu7,
        }
    }
}

/// A parameter for regtest mode.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct RegTestNuParam {
    consensus_branch_id: consensus::BranchId,
    activation_height: BlockHeight,
}

impl TryFrom<String> for RegTestNuParam {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (branch_id, height) = value.split_once(':').ok_or("Invalid `regtest_nuparam`")?;

        let consensus_branch_id = u32::from_str_radix(branch_id, 16)
            .ok()
            .and_then(|branch_id| consensus::BranchId::try_from(branch_id).ok())
            .ok_or("Invalid `regtest_nuparam`")?;

        let activation_height = height
            .parse()
            .map(BlockHeight::from_u32)
            .map_err(|_| "Invalid `regtest_nuparam`")?;

        Ok(Self {
            consensus_branch_id,
            activation_height,
        })
    }
}

impl From<RegTestNuParam> for String {
    fn from(nuparam: RegTestNuParam) -> Self {
        format!(
            "{:08x}:{}",
            u32::from(nuparam.consensus_branch_id),
            nuparam.activation_height
        )
    }
}

pub(crate) mod kind {
    use std::fmt;

    use rusqlite::{
        ToSql,
        types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
    };
    use serde::{Deserializer, Serializer, de::Visitor};
    use zcash_protocol::consensus::NetworkType;

    use super::NetworkKind;

    fn str_to_type(s: &str) -> Option<NetworkType> {
        match s {
            "main" => Some(NetworkType::Main),
            "test" => Some(NetworkType::Test),
            "regtest" => Some(NetworkType::Regtest),
            _ => None,
        }
    }

    fn str_to_kind(s: &str) -> Option<NetworkKind> {
        match s {
            "main" => Some(NetworkKind::Main),
            "test" => Some(NetworkKind::Test),
            "configured-testnet" => Some(NetworkKind::ConfiguredTestnet),
            "regtest" => Some(NetworkKind::Regtest),
            _ => None,
        }
    }

    pub(crate) fn type_to_str(network_type: &NetworkType) -> &'static str {
        match network_type {
            NetworkType::Main => "main",
            NetworkType::Test => "test",
            NetworkType::Regtest => "regtest",
        }
    }

    fn kind_to_str(network_kind: &NetworkKind) -> &'static str {
        match network_kind {
            NetworkKind::Main => "main",
            NetworkKind::Test => "test",
            NetworkKind::ConfiguredTestnet => "configured-testnet",
            NetworkKind::Regtest => "regtest",
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<NetworkKind, D::Error> {
        struct NetworkTypeVisitor;
        impl Visitor<'_> for NetworkTypeVisitor {
            type Value = NetworkKind;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "one of 'main', 'test', 'configured-testnet', or 'regtest'"
                )
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                str_to_kind(v).ok_or_else(|| {
                    serde::de::Error::invalid_type(serde::de::Unexpected::Str(v), &self)
                })
            }
        }

        deserializer.deserialize_str(NetworkTypeVisitor)
    }

    pub(crate) fn serialize<S: Serializer>(
        network_kind: &NetworkKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(kind_to_str(network_kind))
    }

    #[derive(serde::Serialize)]
    pub(crate) struct Serializable(#[serde(with = "crate::network::kind")] pub(crate) NetworkKind);

    pub(crate) struct Sql(pub(crate) NetworkType);

    impl FromSql for Sql {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            str_to_type(value.as_str()?)
                .ok_or(FromSqlError::InvalidType)
                .map(Self)
        }
    }

    impl ToSql for Sql {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::Borrowed(ValueRef::Text(
                type_to_str(&self.0).as_bytes(),
            )))
        }
    }
}
