use revm::primitives::hardfork::SpecId;
use std::str::FromStr;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HlSpecId {
    #[default]
    V1, // V1
}

impl HlSpecId {
    pub const fn is_enabled_in(self, other: HlSpecId) -> bool {
        other as u8 <= self as u8
    }

    /// Converts the [`HlSpecId`] into a [`SpecId`].
    pub const fn into_eth_spec(self) -> SpecId {
        match self {
            Self::V1 => SpecId::CANCUN,
        }
    }
}

impl From<HlSpecId> for SpecId {
    fn from(spec: HlSpecId) -> Self {
        spec.into_eth_spec()
    }
}

/// String identifiers for HL hardforks
pub mod name {
    pub const V1: &str = "V1";
}

impl FromStr for HlSpecId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            name::V1 => Self::V1,
            _ => return Err(format!("Unknown HL spec: {s}")),
        })
    }
}

impl From<HlSpecId> for &'static str {
    fn from(spec_id: HlSpecId) -> Self {
        match spec_id {
            HlSpecId::V1 => name::V1,
        }
    }
}
