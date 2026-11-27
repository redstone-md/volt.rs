//! Compile-time registration of actions, loaders, API handlers and sitemap providers.

use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context::Ctx;
use crate::error::{Error, Result};
use crate::form::FormData;
use crate::response::Response;
use crate::seo::SitemapEntry;
use crate::types::TypeScript;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type Describe = fn(&mut TypeScript) -> String;

/// Request body of an action call.
pub enum Payload {
    Json(Bytes),
    /// Urlencoded or multipart form fields.
    Form(FormData),
}

impl Payload {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        match self {
            Self::Json(body) if body.is_empty() => {
                serde_json::from_slice(b"null").map_err(|e| Error::bad_request(format!("missing input: {e}")))
            }
            Self::Json(body) => {
                serde_json::from_slice(body).map_err(|e| Error::bad_request(format!("invalid input: {e}")))
            }
            Self::Form(form) => form.decode().map_err(|e| Error::bad_request(e.to_string())),
        }
    }
}

/// Return values accepted from actions and loaders.
pub trait Output {
    type Ok;
    fn into_result(self) -> Result<Self::Ok>;
}

impl<T, E: Into<Error>> Output for std::result::Result<T, E> {
    type Ok = T;

    fn into_result(self) -> Result<T> {
        self.map_err(Into::into)
    }
}

pub fn encode<R: Output>(output: R) -> Result<Value>
where
    R::Ok: Serialize,
{
    serde_json::to_value(output.into_result()?).map_err(Error::from)
}

pub struct ActionDef {
    pub name: &'static str,
    pub handler: fn(Ctx, Payload) -> BoxFuture<Result<Value>>,
    pub input: Option<Describe>,
    pub output: Describe,
}

pub struct LoaderDef {
    pub route: &'static str,
    pub handler: fn(Ctx) -> BoxFuture<Result<Value>>,
    pub output: Describe,
}

pub struct ApiDef {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: fn(Ctx) -> BoxFuture<Response>,
}

pub struct SitemapDef {
    pub handler: fn(Ctx) -> BoxFuture<Result<Vec<SitemapEntry>>>,
}

inventory::collect!(ActionDef);
inventory::collect!(LoaderDef);
inventory::collect!(ApiDef);
inventory::collect!(SitemapDef);

pub fn actions() -> impl Iterator<Item = &'static ActionDef> {
    inventory::iter::<ActionDef>.into_iter()
}

pub fn loaders() -> impl Iterator<Item = &'static LoaderDef> {
    inventory::iter::<LoaderDef>.into_iter()
}

pub fn apis() -> impl Iterator<Item = &'static ApiDef> {
    inventory::iter::<ApiDef>.into_iter()
}

pub fn sitemaps() -> impl Iterator<Item = &'static SitemapDef> {
    inventory::iter::<SitemapDef>.into_iter()
}
