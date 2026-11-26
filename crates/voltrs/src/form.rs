//! Lenient `application/x-www-form-urlencoded` deserializer for progressive-enhancement forms.
//! String values are parsed into the field's type; missing booleans become `false`.

use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::forward_to_deserialize_any;

#[derive(Debug)]
pub struct FormError(String);

impl std::fmt::Display for FormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormError {}

impl de::Error for FormError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

/// Parses urlencoded pairs, grouping repeated keys and stripping a trailing `[]`.
pub fn parse_pairs(body: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    for pair in body.split(|b| *b == b'&').filter(|p| !p.is_empty()) {
        let mut parts = pair.splitn(2, |b| *b == b'=');
        let key = decode(parts.next().unwrap_or_default());
        let value = decode(parts.next().unwrap_or_default());
        let key = key.strip_suffix("[]").map(str::to_owned).unwrap_or(key);
        if key.starts_with("$volt") {
            continue;
        }
        match fields.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.push(value),
            None => fields.push((key, vec![value])),
        }
    }
    fields
}

fn decode(raw: &[u8]) -> String {
    let spaced: Vec<u8> = raw.iter().map(|b| if *b == b'+' { b' ' } else { *b }).collect();
    percent_encoding::percent_decode(&spaced).decode_utf8_lossy().into_owned()
}

pub fn from_bytes<T: DeserializeOwned>(body: &[u8]) -> Result<T, FormError> {
    T::deserialize(Form(parse_pairs(body)))
}

struct Form(Vec<(String, Vec<String>)>);

impl<'de> de::Deserializer<'de> for Form {
    type Error = FormError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_map(Fields { fields: self.0.into_iter(), value: None })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        mut self,
        _name: &'static str,
        names: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, FormError> {
        for name in names {
            if !self.0.iter().any(|(k, _)| k == name) {
                self.0.push(((*name).to_owned(), Vec::new()));
            }
        }
        self.deserialize_any(visitor)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}

struct Fields {
    fields: std::vec::IntoIter<(String, Vec<String>)>,
    value: Option<(String, Vec<String>)>,
}

impl<'de> MapAccess<'de> for Fields {
    type Error = FormError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, FormError> {
        match self.fields.next() {
            Some((key, values)) => {
                let out = seed.deserialize(key.clone().into_deserializer()).map(Some);
                self.value = Some((key, values));
                out
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, FormError> {
        let (key, values) = self.value.take().ok_or_else(|| FormError("value without key".into()))?;
        seed.deserialize(Values { key, values })
    }
}

struct Values {
    key: String,
    values: Vec<String>,
}

impl Values {
    fn first(&self) -> Result<&str, FormError> {
        self.values.first().map(String::as_str).ok_or_else(|| FormError(format!("missing field `{}`", self.key)))
    }

    fn parse<T: std::str::FromStr>(&self) -> Result<T, FormError> {
        let raw = self.first()?.trim();
        raw.parse().map_err(|_| FormError(format!("invalid value for `{}`: {raw:?}", self.key)))
    }
}

macro_rules! parse_number {
    ($($method:ident => $visit:ident),+) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
            visitor.$visit(self.parse()?)
        })+
    };
}

impl<'de> de::Deserializer<'de> for Values {
    type Error = FormError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        match self.values.len() {
            1 => visitor.visit_string(self.values.into_iter().next().unwrap_or_default()),
            _ => self.deserialize_seq(visitor),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        let value = self.values.first().map(|v| v.to_ascii_lowercase());
        let truthy = match value.as_deref() {
            None | Some("" | "false" | "off" | "0" | "no") => false,
            Some("true" | "on" | "1" | "yes") => true,
            Some(other) => {
                return Err(FormError(format!("invalid boolean for `{}`: {other:?}", self.key)));
            }
        };
        visitor.visit_bool(truthy)
    }

    parse_number! {
        deserialize_i8 => visit_i8, deserialize_i16 => visit_i16, deserialize_i32 => visit_i32,
        deserialize_i64 => visit_i64, deserialize_i128 => visit_i128, deserialize_u8 => visit_u8,
        deserialize_u16 => visit_u16, deserialize_u32 => visit_u32, deserialize_u64 => visit_u64,
        deserialize_u128 => visit_u128, deserialize_f32 => visit_f32, deserialize_f64 => visit_f64,
        deserialize_char => visit_char
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_string(self.first()?.to_owned())
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        match self.values.first().map(String::as_str) {
            None | Some("") => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_seq(Items { key: self.key, values: self.values.into_iter() })
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, FormError> {
        let value: de::value::StringDeserializer<FormError> = self.first()?.to_owned().into_deserializer();
        visitor.visit_enum(value)
    }

    forward_to_deserialize_any! {
        bytes byte_buf unit_struct tuple tuple_struct map struct identifier ignored_any
    }
}

struct Items {
    key: String,
    values: std::vec::IntoIter<String>,
}

impl<'de> SeqAccess<'de> for Items {
    type Error = FormError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, FormError> {
        match self.values.next() {
            Some(value) => seed.deserialize(Values { key: self.key.clone(), values: vec![value] }).map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Input {
        title: String,
        count: u32,
        done: bool,
        note: Option<String>,
        tags: Vec<String>,
    }

    #[test]
    fn coerces_values() {
        let input: Input = super::from_bytes(b"title=Hello+world&count=3&tags[]=a&tags[]=b&note=").unwrap();
        assert_eq!(
            input,
            Input {
                title: "Hello world".into(),
                count: 3,
                done: false,
                note: None,
                tags: vec!["a".into(), "b".into()]
            }
        );
    }

    #[test]
    fn reports_missing_fields() {
        let error = super::from_bytes::<Input>(b"count=1").unwrap_err();
        assert!(error.to_string().contains("title"));
    }
}
