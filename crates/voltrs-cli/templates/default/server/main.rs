use std::sync::Mutex;

use voltrs::prelude::*;

#[model]
#[derive(Clone)]
struct Todo {
    id: u32,
    title: String,
    done: bool,
}

#[model]
struct Home {
    todos: Vec<Todo>,
    loaded_in_micros: u128,
}

#[model]
struct NewTodo {
    title: String,
}

#[model]
struct ToggleTodo {
    id: u32,
}

struct Store {
    todos: Mutex<Vec<Todo>>,
}

impl Store {
    fn new() -> Self {
        let todos = ["Install volt", "Write a server action", "Ship it"]
            .iter()
            .enumerate()
            .map(|(i, title)| Todo { id: i as u32 + 1, title: (*title).into(), done: i == 0 })
            .collect();
        Self { todos: Mutex::new(todos) }
    }
}

#[loader("/")]
async fn home(ctx: Ctx) -> Result<Home> {
    let todos = ctx.state::<Store>().todos.lock().map_err(|_| Error::internal("store poisoned"))?.clone();
    Ok(Home { todos, loaded_in_micros: ctx.elapsed().as_micros() })
}

#[action]
async fn add_todo(ctx: Ctx, input: NewTodo) -> Result<Todo> {
    let title = input.title.trim();
    if title.is_empty() || title.len() > 120 {
        return Err(Error::bad_request("title must be 1-120 characters"));
    }
    let mut todos = ctx.state::<Store>().todos.lock().map_err(|_| Error::internal("store poisoned"))?;
    let todo = Todo { id: todos.iter().map(|t| t.id).max().unwrap_or(0) + 1, title: title.into(), done: false };
    todos.push(todo.clone());
    Ok(todo)
}

#[action]
async fn toggle_todo(ctx: Ctx, input: ToggleTodo) -> Result<Todo> {
    let mut todos = ctx.state::<Store>().todos.lock().map_err(|_| Error::internal("store poisoned"))?;
    let todo = todos.iter_mut().find(|t| t.id == input.id).ok_or_else(not_found)?;
    todo.done = !todo.done;
    Ok(todo.clone())
}

#[get("/api/health")]
async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

fn main() {
    App::new().state(Store::new()).run();
}
