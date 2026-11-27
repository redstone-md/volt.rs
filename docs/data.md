# Data: loaders, server actions and API routes

The server side of a volt app is Rust. React components never fetch their own backend: pages receive data from **loaders**, and mutations go through **actions**. Both are plain async Rust functions, and their types are exported to TypeScript automatically.

## Models

Types that cross into TypeScript are marked with `#[model]`. It derives `Serialize`, `Deserialize` and `Type`, and renames fields to camelCase.

```rust
use voltrs::prelude::*;

/// A blog post.
#[model]
struct Post {
    slug: String,
    title: String,
    published_at: Option<String>,
    tags: Vec<String>,
}
```

generates

```ts
/** A blog post. */
export interface Post {
  slug: string;
  title: string;
  publishedAt?: string | null;
  tags: string[];
}
```

Enums become string unions, `#[serde(tag = "type")]` enums become discriminated unions, generic structs are inlined. Use `#[derive(Type)]` directly to keep your own serde attributes; volt honors `rename`, `rename_all`, `skip`, `default`, `flatten`, `tag`, `content` and `untagged`.

## Loaders

A loader provides the `data` prop of a route.

```rust
#[loader("/blog/[slug]")]
async fn post(ctx: Ctx) -> Result<Post> {
    let slug = ctx.param("slug").unwrap_or_default();
    let db = ctx.state::<Db>();
    db.post(slug).await?.ok_or_else(not_found)
}
```

```tsx
import type { PageProps } from "@volt/api";

export default function PostPage({ data }: PageProps<"/blog/[slug]">) {
  return <h1>{data.title}</h1>;
}
```

`PageProps<"/blog/[slug]">` is typed from the loader's return type. Loaders run on the initial request and on every client-side navigation to the route. Return `Err(not_found())` to render the not-found page or `Err(redirect("/login"))` to redirect.

## Actions

An action is a mutation callable from React.

```rust
#[model]
struct NewComment {
    post: String,
    body: String,
}

#[action]
async fn add_comment(ctx: Ctx, input: NewComment) -> Result<Comment> {
    if input.body.trim().is_empty() {
        return Err(Error::bad_request("comment is empty"));
    }
    ctx.state::<Db>().insert_comment(input).await
}
```

`add_comment` becomes `addComment` in `@volt/api`:

```ts
export const addComment: Action<NewComment, Comment>;
```

### With forms

```tsx
import { addComment } from "@volt/api";
import Form, { useFormPending } from "volt-rs/form";

<Form action={addComment} onSuccess={(_, form) => form.reset()}>
  <input type="hidden" name="post" value={slug} />
  <textarea name="body" required />
  <button type="submit">Send</button>
</Form>
```

`<Form>` renders a real `<form method="post">`. Without JavaScript the browser posts it, the action runs and the user is redirected back. With JavaScript it submits in place, exposes the pending state through `useFormPending()` and re-runs the route loader afterwards so the page shows fresh data. Form fields are parsed into the input struct: numbers, booleans (checkboxes), optional fields and repeated keys (`tags[]`) are converted to their Rust types.

### From code

```tsx
import { addComment } from "@volt/api";
import { useAction } from "volt-rs/actions";
import { useRouter } from "volt-rs/navigation";

const comment = await addComment({ post, body });
router.refresh();

const { run, pending, error, data } = useAction(addComment, { refresh: true });
```

Errors become an `ActionError` with `status` and `message`. Client errors (4xx) keep their message; server errors show a generic message in production and are logged.

Actions are protected against cross-site requests: calls whose `Origin` does not match the host are rejected.

## API routes

For JSON endpoints, webhooks and anything not driven by React:

```rust
#[get("/api/posts")]
async fn list(ctx: Ctx) -> Result<Json<Vec<Post>>> {
    Ok(Json(ctx.state::<Db>().posts().await?))
}

#[post("/api/webhook/[provider]")]
async fn webhook(ctx: Ctx) -> Result<StatusCode> {
    let event: Event = ctx.json()?;
    Ok(StatusCode::NO_CONTENT)
}
```

`#[get]`, `#[post]`, `#[put]`, `#[patch]` and `#[delete]` take the same path syntax as `app/`. Handlers return anything implementing `IntoResponse`: `Json<T>`, `Html<T>`, `String`, `&str`, `StatusCode`, `(StatusCode, T)`, `Response` or a `Result` of those.

## Ctx

Every loader, action and handler can take `ctx: Ctx` as its first argument.

| Method | Returns |
| --- | --- |
| `ctx.param("slug")` | route param |
| `ctx.param_as::<u32>("id")` | parsed param, 404 if invalid |
| `ctx.query::<T>()` / `ctx.query_param("q")` | query string |
| `ctx.header("user-agent")` / `ctx.headers()` | request headers |
| `ctx.cookie("session")` | cookie value |
| `ctx.json::<T>()` / `ctx.form::<T>()` / `ctx.body()` | request body |
| `ctx.state::<T>()` | shared state |
| `ctx.set_cookie(Cookie::new("session", id))` | adds `Set-Cookie` |
| `ctx.set_header(name, value)` / `ctx.set_status(code)` | response headers and status |
| `ctx.method()`, `ctx.path()`, `ctx.remote_addr()`, `ctx.elapsed()` | request info |
| `ctx.revalidate_path("/blog/x")` / `ctx.revalidate_prefix("/blog")` | purges cached pages, see [static generation](static-generation.md#on-demand-revalidation) |

## State and middleware

```rust
fn main() {
    App::new()
        .state(Db::connect())
        .middleware(|ctx: Ctx| async move {
            if ctx.path().starts_with("/admin") && ctx.cookie("session").is_none() {
                return Some(Response::redirect(StatusCode::SEE_OTHER, "/login"));
            }
            None
        })
        .run();
}
```

Middleware runs before routing for every request; returning `Some(response)` answers immediately.

## Errors

| Rust | HTTP |
| --- | --- |
| `Error::bad_request(msg)` | 400 |
| `Error::unauthorized(msg)` | 401 |
| `Error::forbidden(msg)` | 403 |
| `not_found()` | 404, renders `not-found.tsx` |
| `Error::conflict(msg)` | 409 |
| `redirect("/x")` | 303 (`Error::permanent_redirect` for 308) |
| `?` on any `std::error::Error` | 500 |
