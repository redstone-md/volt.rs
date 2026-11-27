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

fn main() {
    App::new().run();
}
