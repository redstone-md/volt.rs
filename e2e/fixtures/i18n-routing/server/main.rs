use voltrs::prelude::*;

#[model]
struct About {
    locale: String,
    greeting: String,
}

#[loader("/about")]
async fn about(ctx: Ctx) -> Result<About> {
    let greeting = if ctx.locale() == "de" { "Hallo aus Rust" } else { "Hello from Rust" };
    Ok(About { locale: ctx.locale().to_owned(), greeting: greeting.into() })
}

#[loader("/old")]
async fn old(_ctx: Ctx) -> Result<()> {
    Err(redirect("/about"))
}

#[loader("/to-file")]
async fn to_file(_ctx: Ctx) -> Result<()> {
    Err(redirect("/file.txt"))
}

#[loader("/to-api")]
async fn to_api(_ctx: Ctx) -> Result<()> {
    Err(redirect("/api/ping?x=1"))
}

#[get("/api/ping")]
async fn ping(ctx: Ctx) -> String {
    format!("pong:{}", ctx.locale())
}

#[model]
struct Echo {
    text: String,
}

#[model]
struct Echoed {
    locale: String,
    text: String,
}

#[action]
async fn echo(ctx: Ctx, input: Echo) -> Result<Echoed> {
    Ok(Echoed { locale: ctx.locale().to_owned(), text: input.text })
}

#[sitemap]
async fn posts(_ctx: Ctx) -> Result<Vec<SitemapEntry>> {
    Ok(vec![SitemapEntry::new("/blog/from-rust")])
}

fn main() {
    App::new().run();
}
