use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};
use uuid::{Uuid, Variant};

/// 实体 ID 和请求 ID 统一通过此函数生成。
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EntityId(Uuid);

impl EntityId {
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl FromStr for EntityId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id = Uuid::parse_str(value).map_err(|_| "invalid_uuid_v7")?;
        if value.len() != 36 || id.get_version_num() != 7 || id.get_variant() != Variant::RFC4122 {
            return Err("invalid_uuid_v7");
        }
        Ok(Self(id))
    }
}

impl<'de> Deserialize<'de> for EntityId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_v7_with_rfc_variant() {
        assert!(new_id().to_string().parse::<EntityId>().is_ok());
        for invalid in [
            "67e55044-10b1-426f-9247-bb680e5fe0c8",
            "019535d9-3df7-79fb-7466-fa907fa17f9e",
            "019535d93df779fbb466fa907fa17f9e",
            "invalid",
        ] {
            assert!(invalid.parse::<EntityId>().is_err());
        }
    }
}
