//! Bounded JSON ingress with duplicate keys rejected before typed decoding.
use serde::{
    Deserialize,
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
};
use std::{collections::BTreeSet, fmt};

// Check every object, including unknown extension fields, before typed decoding.
// serde's normal recursion limit and the byte bound also apply to this pass.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with no repeated object keys")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<UniqueJson, A::Error> {
                while values.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<UniqueJson, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = values.next_key::<String>()? {
                    if !names.insert(name) {
                        return Err(de::Error::custom("repeated object key"));
                    }
                    values.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub(super) fn decode<T: DeserializeOwned>(
    bytes: &[u8],
    limit: usize,
    what: &str,
) -> Result<T, String> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(format!("{what} is empty or exceeds its byte limit"));
    }
    serde_json::from_slice::<UniqueJson>(bytes).map_err(|_| {
        format!("{what} must contain one complete JSON value with unique object keys")
    })?;
    serde_json::from_slice(bytes)
        .map_err(|_| format!("{what} has an invalid structure or field type"))
}
