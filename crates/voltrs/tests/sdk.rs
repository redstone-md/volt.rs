use voltrs::prelude::*;
use voltrs::sdk::Sdk;

/// A todo item.
#[model]
struct Todo {
    id: u32,
    /// What needs doing.
    title: String,
    done_at: Option<String>,
    tags: Vec<String>,
}

#[model]
struct NewTodo {
    title: String,
}

#[model]
struct Upload {
    file: File,
    cover: Option<File>,
    extras: Vec<File>,
}

#[model]
enum Status {
    Active,
    Archived,
}

#[allow(dead_code)]
#[derive(serde::Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    Created { id: u32 },
    Deleted,
}

#[derive(serde::Serialize, Type)]
struct Page<T> {
    items: Vec<T>,
    total: u64,
}

#[action]
async fn add_todo(_ctx: Ctx, input: NewTodo) -> Result<Todo> {
    Ok(Todo { id: 1, title: input.title, done_at: None, tags: vec![] })
}

#[action]
async fn clear_all() -> Result<()> {
    Ok(())
}

#[action]
fn last_event(_ctx: &Ctx) -> Result<Option<Event>> {
    Ok(None)
}

#[action]
async fn upload(input: Upload) -> Result<u64> {
    Ok(input.file.len() as u64 + input.extras.len() as u64 + u64::from(input.cover.is_some()))
}

#[loader("/todos/[status]")]
async fn todos(ctx: Ctx) -> Result<Page<Todo>> {
    let _ = ctx.param("status");
    Ok(Page { items: vec![], total: 0 })
}

#[loader("/status")]
async fn status() -> Result<Status> {
    Ok(Status::Active)
}

#[get("/api/health")]
async fn health() -> &'static str {
    "ok"
}

#[test]
fn generates_typed_sdk() {
    let sdk = Sdk::generate();
    let source = sdk.source();
    assert!(source.contains("/** A todo item. */\nexport interface Todo {"), "{source}");
    assert!(source.contains("  /** What needs doing. */\n  title: string;"), "{source}");
    assert!(source.contains("  doneAt?: string | null;"), "{source}");
    assert!(source.contains("export type Status = \"Active\" | \"Archived\";"), "{source}");
    assert!(
        source.contains("export type Event = { kind: \"created\"; id: number; } | { kind: \"deleted\" };"),
        "{source}"
    );
    assert!(source.contains("export const addTodo = createAction<NewTodo, Todo>(\"add_todo\");"), "{source}");
    assert!(source.contains("export const clearAll = createAction<void, null>(\"clear_all\");"), "{source}");
    assert!(source.contains("export const lastEvent = createAction<void, Event | null>(\"last_event\");"), "{source}");
    assert!(source.contains("  \"/todos/[status]\": { items: Todo[]; total: number; };"), "{source}");
    assert!(
        source.contains(
            "export interface Upload {\n  file: File | Blob | UploadedFile;\n  cover?: File | Blob | UploadedFile | null;\n  extras: (File | Blob | UploadedFile)[];\n}"
        ),
        "{source}"
    );
    assert!(source.contains("export interface UploadedFile {\n  name: string;\n  filename: string;\n"), "{source}");
    assert_eq!(sdk.info().api, vec!["GET /api/health"]);
}
