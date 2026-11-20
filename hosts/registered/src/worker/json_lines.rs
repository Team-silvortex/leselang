//! Private shared framing for the two portable example carriers.
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::{
    fmt,
    io::{BufRead, Read},
};

/// JSON objects must not contain duplicate fields, including nested objects.
/// Parsing Value directly would discard that evidence before gateway validation.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|v| StrictValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(StrictValue(value)) = a.next_element()? {
                    items.push(value);
                }
                Ok(StrictValue(Value::Array(items)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut fields = Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if fields.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON field"));
                    }
                    fields.insert(key, a.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(fields)))
            }
        }
        d.deserialize_any(Json)
    }
}

pub fn read_record(reader: &mut impl BufRead, limit: usize) -> Result<Value, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| format!("read JSON record: {e}"))?;
        if available.is_empty() {
            return Err("JSON record closed or truncated".into());
        }
        let length = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if bytes.len() + length > limit {
            return Err("JSON record exceeds wire limit".into());
        }
        let complete = available[length - 1] == b'\n';
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if complete {
            return serde_json::from_slice::<StrictValue>(&bytes)
                .map(|value| value.0)
                .map_err(|_| "malformed JSON record".into());
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Correlation {
    version: u64,
    window: String,
    epoch: String,
    session: String,
    serial: u64,
}
impl Correlation {
    pub fn read(value: &Value) -> Option<Self> {
        Some(Self {
            version: value["transport_version"].as_u64()?,
            window: value["window"].as_str()?.into(),
            epoch: value["epoch"].as_str()?.into(),
            session: value["session"].as_str()?.into(),
            serial: value["request_id"].as_u64()?,
        })
    }
}

pub fn challenge() -> Result<String, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut bytes = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut bytes))
            .map_err(|_| "cannot obtain operating-system request randomness")?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err("the Node reference transport currently supports Linux and macOS".into())
}
