use voltrs::prelude::*;

#[model]
struct Dashboard {
    user: Option<String>,
}

/// Reads the request header that `middleware.ts` adds.
#[loader("/dashboard")]
async fn dashboard(ctx: Ctx) -> Result<Dashboard> {
    Ok(Dashboard { user: ctx.header("x-user").map(str::to_owned) })
}

/// Answers before `app/api/[name]/route.ts`.
#[get("/api/rust")]
async fn rust(_ctx: Ctx) -> Json<Value> {
    Json(json!({ "from": "rust" }))
}

fn main() {
    App::new()
        .middleware(|ctx: Ctx| async move {
            ctx.set_header("x-rust-middleware", "1");
            None
        })
        .run();
}
