use voltrs::prelude::*;

#[model]
struct Profile {
    name: String,
    language: String,
}

#[loader("/profile")]
async fn profile() -> Result<Profile> {
    Ok(Profile { name: "Ferris".into(), language: "Rust".into() })
}

#[model]
struct Who {
    user: String,
}

#[loader("/who")]
async fn who(ctx: Ctx) -> Result<Who> {
    Ok(Who { user: ctx.cookie("user").unwrap_or_else(|| "nobody".into()) })
}

fn main() {
    App::new().run();
}
