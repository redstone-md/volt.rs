//! volt.rs runtime: a Rust server for React apps with SSR, server actions, loaders and API routes.
//!
//! ```no_run
//! use voltrs::prelude::*;
//!
//! #[model]
//! struct Greeting {
//!     message: String,
//! }
//!
//! #[loader("/")]
//! async fn home(_ctx: Ctx) -> Result<Greeting> {
//!     Ok(Greeting { message: "Hello from Rust".into() })
//! }
//!
//! fn main() {
//!     App::new().run();
//! }
//! ```

mod app;
mod assets;
mod cache;
mod context;
mod dev;
mod document;
mod error;
mod file;
#[cfg(feature = "images")]
mod images;
mod multipart;
mod response;
mod server;

pub mod config;
pub mod form;
pub mod log;
pub mod manifest;
pub mod registry;
pub mod router;
pub mod sdk;
pub mod seo;
pub mod ssr;
pub mod types;

pub use app::{App, Prerendered, StaticRenderer};
pub use cache::{revalidate_path, revalidate_prefix};
pub use context::Ctx;
pub use error::{Error, ErrorKind, FieldErrors, Result, not_found, redirect};
pub use file::File;
pub use form::{FormData, FormValue};
pub use response::{Cookie, Html, IntoResponse, Json, Response, SameSite};
pub use seo::{ChangeFrequency, SitemapEntry};
pub use types::Type;
pub use voltrs_macros::{Type, action, delete, get, loader, model, patch, post, put, sitemap};

pub use http::{self, HeaderMap, Method, StatusCode};
pub use serde;
pub use serde_json;

pub mod prelude {
    pub use crate::{
        App, ChangeFrequency, Cookie, Ctx, Error, File, FormData, Html, IntoResponse, Json, Response, Result, SameSite,
        SitemapEntry, StatusCode, Type, action, delete, get, loader, model, not_found, patch, post, put, redirect,
        revalidate_path, revalidate_prefix, sitemap,
    };
    pub use serde_json::{Value, json};
}

#[doc(hidden)]
pub mod __private {
    pub use crate::registry::{ActionDef, ApiDef, BoxFuture, LoaderDef, Output, Payload, SitemapDef, encode};
    pub use crate::response::IntoResponse;
    pub use crate::types::{Type, TypeScript, array_of};
    pub use inventory;
}
