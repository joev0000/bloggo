//! The value module contains the [Value] enum type, and a handful of useful
//! trait implementations.

use crate::{error::Error, Result};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::collections::BTreeMap;

/// An unsigned number, either an integer or floating point number.
#[derive(Debug)]
pub enum Number {
    Integer(i64),
    Float(f64),
}

/// A value parsed from post front matter. This enum is necessary since
/// each front matter type (YAML, TOML, etc.) is different.
#[derive(Debug)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// Return [Some]([String]) if the Value is a string, [None] otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use bloggo::Value;
    ///
    /// let string  = Value::String("a string".to_string());
    /// let boolean = Value::Boolean(true);
    ///
    /// assert_eq!(Some("a string".to_string()), string.as_string());
    /// assert_eq!(None, boolean.as_string());
    /// ```
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(String::from(s))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Boolean(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Number(Number::Integer(i))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Number(Number::Float(f))
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Value {
        Value::Array(v)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(m: BTreeMap<String, Value>) -> Value {
        Value::Map(m)
    }
}

impl TryFrom<serde_yaml::Value> for Value {
    type Error = Error;

    fn try_from(yval: serde_yaml::Value) -> Result<Value> {
        match yval {
            serde_yaml::Value::Null => Ok(Value::Null),
            serde_yaml::Value::Bool(b) => Ok(Value::Boolean(b)),
            serde_yaml::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    Ok(Value::Number(Number::Float(f)))
                } else if let Some(i) = n.as_i64() {
                    Ok(Value::Number(Number::Integer(i)))
                } else {
                    Err(Error::Other(format!(
                        "Unknown number format while parsing YAML: {}",
                        n
                    )))
                }
            }
            serde_yaml::Value::String(s) => Ok(Value::String(s)),
            serde_yaml::Value::Sequence(s) => {
                let mut vec = Vec::with_capacity(s.len());
                for yv in s {
                    let bv: Value = yv.try_into()?;
                    vec.push(bv);
                }
                Ok(Value::Array(vec))
            }
            serde_yaml::Value::Mapping(m) => {
                let mut map = BTreeMap::new();
                for (k, v) in m.iter() {
                    if let Some(key) = k.as_str() {
                        let value: Value = v.to_owned().try_into()?;
                        map.insert(String::from(key), value);
                    }
                }
                Ok(Value::Map(map))
            }
            serde_yaml::Value::Tagged(tv) => {
                let v: Value = tv.value.try_into()?;
                Ok(v)
            }
        }
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::String(s) => serializer.serialize_str(s),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(Number::Integer(i)) => serializer.serialize_i64(*i),
            Value::Number(Number::Float(f)) => serializer.serialize_f64(*f),
            Value::Array(v) => {
                let mut s = serializer.serialize_seq(Some(v.len()))?;
                for e in v {
                    s.serialize_element(e)?;
                }
                s.end()
            }
            Value::Map(m) => {
                let mut s = serializer.serialize_map(Some(m.len()))?;
                for (k, v) in m.iter() {
                    s.serialize_entry(k, v)?;
                }
                s.end()
            }
            Value::Null => serializer.serialize_none(),
        }
    }
}
