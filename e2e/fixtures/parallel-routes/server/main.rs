use voltrs::prelude::*;

#[model]
struct Photo {
    id: u32,
    title: String,
    /// Which loader produced the data: the full page or the modal slot.
    source: String,
}

fn photo(ctx: &Ctx, source: &str) -> Result<Photo> {
    let id: u32 = ctx.param_as("id")?;
    if !(1..=6).contains(&id) {
        return Err(not_found());
    }
    Ok(Photo { id, title: format!("Photo {id}"), source: source.into() })
}

#[loader("/photos/[id]")]
async fn photo_page(ctx: Ctx) -> Result<Photo> {
    photo(&ctx, "page")
}

#[loader("/@modal/(.)photos/[id]")]
async fn photo_modal(ctx: Ctx) -> Result<Photo> {
    photo(&ctx, "modal")
}

#[loader("/err/@side")]
async fn err_side(_ctx: Ctx) -> Result<Photo> {
    Err(not_found())
}

#[loader("/err/@boom")]
async fn err_boom(_ctx: Ctx) -> Result<Photo> {
    Err(Error::conflict("boom").public())
}

fn main() {
    App::new().run();
}
