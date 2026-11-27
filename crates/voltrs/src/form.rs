//! Lenient form deserializer for progressive-enhancement forms, over `application/x-www-form-urlencoded`
//! and `multipart/form-data` bodies. String values are parsed into the field's type; missing booleans
//! become `false`; file parts deserialize into [`File`].

use bytes::Bytes;
use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::forward_to_deserialize_any;

use crate::file::{self, File};

#[derive(Debug)]
pub struct FormError(String);

impl FormError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

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
    let mut fields = Fields::default();
    for pair in body.split(|b| *b == b'&').filter(|p| !p.is_empty()) {
        let mut parts = pair.splitn(2, |b| *b == b'=');
        let key = decode(parts.next().unwrap_or_default());
        let value = decode(parts.next().unwrap_or_default());
        fields.push(key, value);
    }
    fields.0
}

fn decode(raw: &[u8]) -> String {
    let spaced: Vec<u8> = raw.iter().map(|b| if *b == b'+' { b' ' } else { *b }).collect();
    percent_encoding::percent_decode(&spaced).decode_utf8_lossy().into_owned()
}

/// Ordered fields with repeated keys grouped, skipping framework-internal keys.
struct Fields<T>(Vec<(String, Vec<T>)>);

impl<T> Default for Fields<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Fields<T> {
    fn push(&mut self, key: String, value: T) {
        let key = key.strip_suffix("[]").map(str::to_owned).unwrap_or(key);
        if key.starts_with("$volt") || key.starts_with("$ACTION") {
            return;
        }
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.push(value),
            None => self.0.push((key, vec![value])),
        }
    }
}

pub fn from_bytes<T: DeserializeOwned>(body: &[u8]) -> Result<T, FormError> {
    FormData::urlencoded(body).decode()
}

/// A submitted form field value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormValue {
    Text(String),
    File(File),
}

/// Parsed form body (urlencoded or multipart) with text fields and uploaded files.
#[derive(Debug, Clone, Default)]
pub struct FormData {
    fields: Vec<(String, Vec<FormValue>)>,
}

impl FormData {
    pub fn urlencoded(body: &[u8]) -> Self {
        let fields = parse_pairs(body)
            .into_iter()
            .map(|(key, values)| (key, values.into_iter().map(FormValue::Text).collect()))
            .collect();
        Self { fields }
    }

    pub fn multipart(content_type: &str, body: &Bytes) -> Result<Self, FormError> {
        let mut fields = Fields::default();
        for part in crate::multipart::parse(content_type, body)? {
            let value = match part.filename {
                Some(filename) => FormValue::File(File {
                    name: part.name.clone(),
                    filename,
                    content_type: part.content_type.unwrap_or_else(|| "application/octet-stream".into()),
                    bytes: part.data,
                }),
                None => FormValue::Text(String::from_utf8_lossy(&part.data).into_owned()),
            };
            fields.push(part.name, value);
        }
        Ok(Self { fields: fields.0 })
    }

    /// Parses a form body by content type; `None` when the body is not a form.
    pub fn from_request(content_type: &str, body: &Bytes) -> Result<Option<Self>, FormError> {
        let essence = content_type.split(';').next().unwrap_or_default().trim();
        if essence.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
            Ok(Some(Self::urlencoded(body)))
        } else if essence.eq_ignore_ascii_case("multipart/form-data") {
            Self::multipart(content_type, body).map(Some)
        } else {
            Ok(None)
        }
    }

    /// All fields in submission order, with repeated keys grouped.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[FormValue])> {
        self.fields.iter().map(|(key, values)| (key.as_str(), values.as_slice()))
    }

    fn values(&self, name: &str) -> impl Iterator<Item = &FormValue> {
        self.fields.iter().filter(move |(key, _)| key == name).flat_map(|(_, values)| values)
    }

    /// First text value of a field.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.values(name).find_map(|v| match v {
            FormValue::Text(text) => Some(text.as_str()),
            FormValue::File(_) => None,
        })
    }

    /// First file of a field.
    pub fn file(&self, name: &str) -> Option<&File> {
        self.files(name).next()
    }

    /// Every file of a field (`<input type="file" multiple>`).
    pub fn files(&self, name: &str) -> impl Iterator<Item = &File> {
        self.values(name).filter_map(|v| match v {
            FormValue::File(file) => Some(file),
            FormValue::Text(_) => None,
        })
    }

    /// Deserializes the fields into `T` with the lenient form rules.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, FormError> {
        T::deserialize(Form(self.fields.clone()))
    }
}

struct Form(Vec<(String, Vec<FormValue>)>);

impl<'de> de::Deserializer<'de> for Form {
    type Error = FormError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_map(FieldAccess { fields: self.0.into_iter(), value: None })
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

struct FieldAccess {
    fields: std::vec::IntoIter<(String, Vec<FormValue>)>,
    value: Option<(String, Vec<FormValue>)>,
}

impl<'de> MapAccess<'de> for FieldAccess {
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
    values: Vec<FormValue>,
}

impl Values {
    fn first(&self) -> Result<&str, FormError> {
        match self.values.first() {
            Some(FormValue::Text(text)) => Ok(text),
            Some(FormValue::File(_)) => Err(FormError(format!("expected text for `{}`, got a file", self.key))),
            None => Err(FormError(format!("missing field `{}`", self.key))),
        }
    }

    fn parse<T: std::str::FromStr>(&self) -> Result<T, FormError> {
        let raw = self.first()?.trim();
        raw.parse().map_err(|_| FormError(format!("invalid value for `{}`: {raw:?}", self.key)))
    }

    /// Takes the single file value, or gives the values back untouched.
    fn into_file(mut self) -> Result<File, Self> {
        match self.values.as_slice() {
            [FormValue::File(_)] => match self.values.pop() {
                Some(FormValue::File(file)) => Ok(file),
                _ => Err(self),
            },
            _ => Err(self),
        }
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
        if self.values.len() != 1 {
            return self.deserialize_seq(visitor);
        }
        match self.into_file() {
            Ok(file) => visitor.visit_map(FileAccess { file, field: 0 }),
            Err(values) => visitor.visit_string(values.first()?.to_owned()),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        let value = match self.values.first() {
            None => None,
            Some(_) => Some(self.first()?.to_ascii_lowercase()),
        };
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
        match self.values.first() {
            None => visitor.visit_none(),
            Some(FormValue::Text(text)) if text.is_empty() => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, FormError> {
        visitor.visit_seq(Items { key: self.key, values: self.values.into_iter() })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, FormError> {
        let key = self.key.clone();
        match self.into_file() {
            Ok(file) => visitor.visit_map(FileAccess { file, field: 0 }),
            Err(values) if values.values.is_empty() => Err(FormError(format!("missing field `{key}`"))),
            Err(_) if name == "File" => Err(FormError(format!("expected a file for `{key}`"))),
            Err(values) => values.deserialize_any(visitor),
        }
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
        bytes byte_buf unit_struct tuple tuple_struct map identifier ignored_any
    }
}

/// Presents a [`File`] as a map with the keys of [`file::FIELDS`].
struct FileAccess {
    file: File,
    field: usize,
}

impl<'de> MapAccess<'de> for FileAccess {
    type Error = FormError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, FormError> {
        match file::FIELDS.get(self.field) {
            Some(key) => seed.deserialize((*key).into_deserializer()).map(Some),
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, FormError> {
        self.field += 1;
        match self.field {
            1 => seed.deserialize(std::mem::take(&mut self.file.name).into_deserializer()),
            2 => seed.deserialize(std::mem::take(&mut self.file.filename).into_deserializer()),
            3 => seed.deserialize(std::mem::take(&mut self.file.content_type).into_deserializer()),
            _ => seed.deserialize(de::value::BytesDeserializer::new(&self.file.bytes)),
        }
    }
}

struct Items {
    key: String,
    values: std::vec::IntoIter<FormValue>,
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
    use bytes::Bytes;
    use serde::Deserialize;

    use super::FormData;
    use crate::file::File;

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

    #[derive(Deserialize, Debug)]
    struct Upload {
        title: String,
        file: File,
        cover: Option<File>,
        extra: Option<File>,
        files: Vec<File>,
        done: bool,
    }

    fn multipart(parts: &[(&str, Option<&str>, &str)]) -> FormData {
        let mut body = String::new();
        for (name, filename, data) in parts {
            body.push_str("--b\r\nContent-Disposition: form-data; name=\"");
            body.push_str(name);
            body.push('"');
            if let Some(filename) = filename {
                body.push_str(&format!("; filename=\"{filename}\"\r\nContent-Type: image/png"));
            }
            body.push_str("\r\n\r\n");
            body.push_str(data);
            body.push_str("\r\n");
        }
        body.push_str("--b--\r\n");
        FormData::multipart("multipart/form-data; boundary=b", &Bytes::from(body)).unwrap()
    }

    #[test]
    fn decodes_files() {
        let form = multipart(&[
            ("title", None, "Hi"),
            ("file", Some("a.png"), "PNG"),
            ("cover", Some("c.png"), "C"),
            ("extra", Some(""), ""),
            ("files[]", Some("1.png"), "1"),
            ("files[]", Some("2.png"), "22"),
            ("done", None, "on"),
        ]);
        let upload: Upload = form.decode().unwrap();
        assert_eq!(upload.title, "Hi");
        assert_eq!((upload.file.name.as_str(), upload.file.filename.as_str()), ("file", "a.png"));
        assert_eq!(upload.file.content_type, "image/png");
        assert_eq!(&upload.file.bytes[..], b"PNG");
        assert_eq!(upload.cover.map(|f| f.len()), Some(1));
        assert!(upload.extra.is_none());
        assert_eq!(upload.files.iter().map(File::len).collect::<Vec<_>>(), [1, 2]);
        assert!(upload.done);
        assert_eq!(form.text("title"), Some("Hi"));
        assert_eq!(form.files("files").count(), 2);
    }

    #[test]
    fn rejects_text_for_files() {
        let form = multipart(&[("title", None, "Hi"), ("file", None, "not a file")]);
        let error = form.decode::<Upload>().unwrap_err().to_string();
        assert!(error.contains("expected a file for `file`"), "{error}");
        let missing = multipart(&[("title", None, "Hi")]).decode::<Upload>().unwrap_err().to_string();
        assert!(missing.contains("missing field `file`"), "{missing}");
    }
}
