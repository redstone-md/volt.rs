use voltrs::prelude::*;

#[model]
struct Who {
    user: Option<String>,
    query: Option<String>,
}

/// Rendered once for everyone (`dynamic = "force-static"`), so it must never see a visitor's cookies.
#[loader("/who")]
async fn who(ctx: Ctx) -> Result<Who> {
    Ok(Who { user: ctx.cookie("user"), query: ctx.query_param("q") })
}

fn main() {
    App::new().run();
}
