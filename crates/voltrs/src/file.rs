//! Uploaded files from `multipart/form-data` bodies.

use std::fmt;

use bytes::Bytes;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

use crate::types::{Type, TypeScript};

/// A file received from a form field. Use it as a `#[model]` field (`File`, `Option<File>`, `Vec<File>`)
/// or read it with [`crate::Ctx::multipart`].
#[derive(Clone, Default, PartialEq, Eq)]
pub struct File {
    /// Form field name.
    pub name: String,
    /// File name sent by the browser, without any directory part.
    pub filename: String,
    pub content_type: String,
    pub bytes: Bytes,
}

impl File {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            .field("name", &self.name)
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("len", &self.bytes.len())
            .finish()
    }
}

pub(crate) const FIELDS: &[&str] = &["name", "filename", "contentType", "bytes"];

/// Serializes metadata only, so a `File` echoed back in a model does not ship its contents.
impl Serialize for File {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("File", 4)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("filename", &self.filename)?;
        out.serialize_field("contentType", &self.content_type)?;
        out.serialize_field("size", &self.bytes.len())?;
        out.end()
    }
}

impl<'de> Deserialize<'de> for File {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_struct("File", FIELDS, FileVisitor)
    }
}

struct FileVisitor;

impl<'de> Visitor<'de> for FileVisitor {
    type Value = File;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an uploaded file")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<File, A::Error> {
        let mut file = File::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "name" => file.name = map.next_value()?,
                "filename" => file.filename = map.next_value()?,
                "contentType" | "content_type" => file.content_type = map.next_value()?,
                "bytes" => file.bytes = map.next_value::<ByteBuf>()?.0,
                _ => {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
        }
        Ok(file)
    }
}

struct ByteBuf(Bytes);

impl<'de> Deserialize<'de> for ByteBuf {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_byte_buf(ByteBufVisitor)
    }
}

struct ByteBufVisitor;

impl<'de> Visitor<'de> for ByteBufVisitor {
    type Value = ByteBuf;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("file contents")
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<ByteBuf, E> {
        Ok(ByteBuf(Bytes::copy_from_slice(v)))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<ByteBuf, E> {
        Ok(ByteBuf(Bytes::from(v)))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<ByteBuf, E> {
        self.visit_bytes(v.as_bytes())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ByteBuf, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element::<u8>()? {
            out.push(byte);
        }
        Ok(ByteBuf(out.into()))
    }
}

/// Inputs take a browser `File` or `Blob`; outputs carry the serialized metadata (`UploadedFile`).
impl Type for File {
    fn ts(ts: &mut TypeScript) -> String {
        let uploaded = ts.named("UploadedFile", |_| {
            "/** Metadata of an uploaded file returned by the server; the contents are never sent back. */\n\
             export interface UploadedFile {\n  name: string;\n  filename: string;\n  contentType: string;\n  size: number;\n}"
                .into()
        });
        format!("File | Blob | {uploaded}")
    }
}
