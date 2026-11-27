use std::sync::Mutex;
use std::time::Duration;

use voltrs::prelude::*;

#[model]
#[derive(Clone)]
struct Uploaded {
    title: String,
    filename: String,
    content_type: String,
    size: usize,
    extras: usize,
}

#[model]
struct Upload {
    title: String,
    file: File,
    extras: Vec<File>,
}

#[model]
struct Home {
    last: Option<Uploaded>,
}

#[model]
struct Signup {
    name: String,
    age: Option<u32>,
}

#[model]
struct Greeting {
    message: String,
    count: u32,
}

#[model]
struct NewItem {
    title: String,
}

#[model]
struct Items {
    items: Vec<String>,
}

#[model]
struct Login {
    name: String,
}

#[model]
struct Me {
    user: Option<String>,
}

#[derive(Default)]
struct Store {
    last: Mutex<Option<Uploaded>>,
    greetings: Mutex<u32>,
    items: Mutex<Vec<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| Error::internal("store poisoned"))
}

#[loader("/")]
async fn home(ctx: Ctx) -> Result<Home> {
    Ok(Home { last: lock(&ctx.state::<Store>().last)?.clone() })
}

#[action]
async fn upload(ctx: Ctx, input: Upload) -> Result<Uploaded> {
    // Long enough for the test to observe the pending state.
    std::thread::sleep(Duration::from_millis(400));
    let uploaded = Uploaded {
        title: input.title,
        filename: input.file.filename.clone(),
        content_type: input.file.content_type.clone(),
        size: input.file.len(),
        extras: input.extras.len(),
    };
    *lock(&ctx.state::<Store>().last)? = Some(uploaded.clone());
    Ok(uploaded)
}

#[action]
async fn greet(ctx: Ctx, input: Signup) -> Result<Greeting> {
    let mut errors = Vec::new();
    if input.name.trim().is_empty() {
        errors.push(("name", "Name is required"));
    }
    if input.age.is_some_and(|age| age < 18) {
        errors.push(("age", "Must be 18 or older"));
    }
    if !errors.is_empty() {
        return Err(Error::validation(errors));
    }
    let mut count = lock(&ctx.state::<Store>().greetings)?;
    *count += 1;
    Ok(Greeting { message: format!("Hello, {}!", input.name.trim()), count: *count })
}

#[loader("/items")]
async fn items(ctx: Ctx) -> Result<Items> {
    Ok(Items { items: lock(&ctx.state::<Store>().items)?.clone() })
}

#[action]
async fn add_item(ctx: Ctx, input: NewItem) -> Result<()> {
    lock(&ctx.state::<Store>().items)?.push(input.title);
    ctx.revalidate_path("/items");
    Ok(())
}

#[action]
async fn login(ctx: Ctx, input: Login) -> Result<()> {
    ctx.set_cookie(Cookie::new("user", input.name).path("/"));
    Ok(())
}

#[loader("/me")]
async fn me(ctx: Ctx) -> Result<Me> {
    Ok(Me { user: ctx.cookie("user") })
}

#[post("/api/upload")]
async fn api_upload(ctx: Ctx) -> Result<Json<Value>> {
    let form = ctx.multipart()?;
    let file = form.file("file").ok_or_else(|| Error::bad_request("missing file"))?;
    Ok(Json(json!({ "note": form.text("note"), "filename": file.filename, "size": file.len() })))
}

fn main() {
    App::new().state(Store::default()).run();
}
