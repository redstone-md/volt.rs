//! Carries the result of a no-JavaScript action submission across its redirect, so the next render of
//! the page can hand it to `useActionState` (post/redirect/get instead of rendering at the action URL).

use serde_json::{Value, json};

use crate::context::Ctx;
use crate::response::Cookie;

const COOKIE: &str = "__volt_action";
/// Percent-encoded size budget, below the 4 KB browsers keep per cookie.
const MAX_ENCODED: usize = 3800;
const MAX_AGE_SECS: i64 = 60;

/// One-time action result read by the next page render.
pub(crate) struct ActionFlash;

impl ActionFlash {
    /// Stores a successful result; `false` when it is too large for a cookie.
    pub fn data(ctx: &Ctx, action: &str, data: &Value) -> bool {
        Self::store(ctx, json!({ "action": action, "data": data }))
    }

    /// Stores a failed result; `false` when it is too large for a cookie.
    pub fn error(ctx: &Ctx, action: &str, error: &Value) -> bool {
        Self::store(ctx, json!({ "action": action, "error": error }))
    }

    fn store(ctx: &Ctx, value: Value) -> bool {
        let cookie = Cookie::new(COOKIE, value.to_string()).max_age(MAX_AGE_SECS);
        let fits = cookie.to_header().len() <= MAX_ENCODED;
        if fits {
            ctx.set_cookie(cookie);
        }
        fits
    }

    /// Reads and clears the pending result, ignoring anything that is not a result this module wrote.
    pub fn take(ctx: &Ctx) -> Option<Value> {
        let raw = ctx.cookie(COOKIE)?;
        ctx.remove_cookie(COOKIE);
        let value: Value = serde_json::from_str(&raw).ok()?;
        let valid = value.get("action").is_some_and(Value::is_string)
            && (value.get("data").is_some() || value.get("error").is_some_and(Value::is_object));
        valid.then_some(value)
    }
}
