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

`<Form>` renders a real `<form method="post" enctype="multipart/form-data">`. Without JavaScript the browser posts it, the action runs and the user is redirected back (to a hidden `redirect` field if present, else the referring page). With JavaScript it submits in place, exposes the pending state through `useFormPending()` and `useFormStatus()` from `react-dom`, and re-runs the route loader afterwards so the page shows fresh data (`refresh={false}` turns that off). Form fields are parsed into the input struct: numbers, booleans (checkboxes), optional fields, repeated keys (`tags[]`) and files are converted to their Rust types. Pass `encType` to override the encoding of no-JavaScript posts.

`<Form>` also accepts a plain function, such as a `useActionState` dispatcher, and then renders an ordinary React form action.

### File uploads

Add `File` fields to the input model:

```rust
#[model]
struct NewPhoto {
    caption: String,
    photo: File,              // required
    thumbnail: Option<File>,  // a file input left empty becomes None
    attachments: Vec<File>,   // <input type="file" multiple>
}

#[action]
async fn add_photo(ctx: Ctx, input: NewPhoto) -> Result<Photo> {
    let File { filename, content_type, bytes, .. } = input.photo;
    ctx.state::<Storage>().save(&filename, &content_type, bytes).await
}
```

```tsx
<Form action={addPhoto}>
  <input name="caption" />
  <input type="file" name="photo" accept="image/*" required />
  <input type="file" name="attachments" multiple />
  <button type="submit">Upload</button>
</Form>
```

`File` has `name` (the field), `filename` (without directory), `content_type` and `bytes` (`bytes::Bytes`, no copy of the request body), plus `len()`. In TypeScript the field is `File | Blob`, so `addPhoto({ caption, photo: blob })` works from code as well: inputs that contain files, and `FormData` with files, are sent as `multipart/form-data`; everything else stays urlencoded or JSON. In a multipart input, nested objects (and arrays of objects) are sent as JSON text fields and decoded on the server, so `upload({ file, meta: { tags } })` works; files must be top-level fields (or arrays of files). Serializing a `File` (for example when it is echoed back) produces its metadata and `size`, never the contents, typed as `UploadedFile` (`{ name, filename, contentType, size }`) in `@volt/api`.

`filename` is what the browser sent, reduced to its last path segment without control characters (`.` and `..` become empty). It is still user input: do not use it as a path on disk without your own naming scheme.

Request bodies, uploads included, are limited by `[server] body_limit_mb` (default 8); larger requests get `413 Payload Too Large`.

### From code

```tsx
import { addComment } from "@volt/api";
import { useAction } from "volt-rs/actions";
import { useRouter } from "volt-rs/navigation";

const comment = await addComment({ post, body });
router.refresh();

const { run, pending, error, data, fieldErrors } = useAction(addComment, { refresh: true });
```

Errors become an `ActionError` with `status`, `message` and, for validation errors, `fieldErrors`. Client errors (4xx) keep their message; server errors show a generic message in production and are logged.

Actions are protected against cross-site requests: calls whose `Origin` does not match the host are rejected.

### useActionState and useFormStatus

Actions follow the React 19 calling convention: when called with two arguments (`previousState, formData`) the last one is the input, so React's own hooks work with them directly:

```tsx
import { useActionState } from "react";
import { useFormStatus } from "react-dom";

function Submit() {
  const { pending } = useFormStatus();
  return <button disabled={pending}>Send</button>;
}

const [comment, formAction] = useActionState(addComment, null);
<form action={formAction}>...<Submit /></form>;
```

`volt-rs/actions` exports a `useActionState` with the same signature that also catches `ActionError`: it keeps the previous state and returns the error as a fourth element, so validation errors render next to the fields instead of reaching the error boundary.

```tsx
import { useActionState } from "volt-rs/actions";

const [comment, formAction, pending, error] = useActionState(addComment, null);

<form action={formAction}>
  <textarea name="body" />
  {error?.fieldErrors?.body?.map((message) => <p key={message}>{message}</p>)}
  <button disabled={pending}>Send</button>
</form>;
```

`<form action={addComment}>` and forms using a `useActionState` dispatcher are server-rendered with the action's URL, so they post (multipart) without JavaScript as well; the action runs and the user is redirected back (303). With `useActionState` from `volt-rs/actions` the result comes back too: the redirected page renders with the returned state, or with the `error` (field errors included) and the initial state. The result travels in a one-time `__volt_action` cookie (up to about 3 KB of JSON; larger results only redirect), every form on the page that uses the same action receives it, and that page render skips the server page cache. A `permalink` (`useActionState(action, null, "/signup")`) makes the form post to that page URL instead; volt runs the action there and redirects back to it. The previous state is not sent to the Rust action. React's own `useActionState` posts without JavaScript too, but does not get the result back.

### Validation errors

```rust
#[action]
async fn sign_up(input: SignUp) -> Result<User> {
    let mut errors = Vec::new();
    if input.name.trim().is_empty() {
        errors.push(("name", "Name is required"));
    }
    if input.password.len() < 8 {
        errors.push(("password", "At least 8 characters"));
    }
    if !errors.is_empty() {
        return Err(Error::validation(errors));
    }
    /* ... */
}
```

`Error::validation` answers `422` with `{ "error": { "status": 422, "message": "Validation failed", "fieldErrors": { "name": ["Name is required"] } } }`. Repeated fields collect several messages. In React it is `ActionError.fieldErrors`, `useAction().fieldErrors` and the `error` of `useActionState`. A no-JavaScript post from a `useActionState` form shows them in the form (see above); other forms show them on a plain error page with a link back. The `redirect` field and the referer are only followed to local paths on the same origin, anything else returns to `/`.

### Revalidation

An action (or API route) can tell the app which data it changed:

```rust
#[action]
async fn add_comment(ctx: Ctx, input: NewComment) -> Result<Comment> {
    let comment = ctx.state::<Db>().insert_comment(&input).await?;
    ctx.revalidate_path(&format!("/blog/{}", input.post)); // a path, or a pattern like "/blog/[slug]"
    Ok(comment)
}
```

| Rust | Effect |
| --- | --- |
| `ctx.revalidate_path("/blog/[slug]")` | drops server-cached pages (prerendered or `revalidate`) whose path matches, and sends an `x-volt-revalidate` hint |
| `ctx.revalidate_prefix("/blog")` | the same for a path and everything below it |
| `ctx.revalidate()` | the same for every path |
| `ctx.set_cookie(...)` in an action | tells the client to drop all prefetched route data and refresh the current route, so the next navigation sees the new cookie; the server cache is shared between users and is not purged |

The client runtime reads the hint after every action call: it drops prefetched route data for matching paths and, if the current page matches, re-runs its loader before the action's promise resolves. `<Form>` and `useAction({ refresh: true })` then skip their own refresh, so the loader runs once.

## API routes

For JSON endpoints, webhooks and anything not driven by React:

```rust
#[get("/api/posts")]
async fn list(ctx: Ctx) -> Result<Json<Vec<Post>>> {
    Ok(Json(ctx.state::<Db>().posts().await?))
}

#[post("/api/avatar")]
async fn avatar(ctx: Ctx) -> Result<Json<Value>> {
    let form = ctx.multipart()?;
    let file = form.file("avatar").ok_or_else(|| Error::bad_request("missing avatar"))?;
    Ok(Json(json!({ "size": file.len(), "type": file.content_type })))
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
| `ctx.json::<T>()` / `ctx.form::<T>()` / `ctx.body()` | request body (`form` reads multipart, files included, and any other content type as urlencoded) |
| `ctx.multipart()` | `FormData`: `text("note")`, `file("photo")`, `files("attachments")`, `iter()`, `decode::<T>()` |
| `ctx.state::<T>()` | shared state |
| `ctx.set_cookie(Cookie::new("session", id))` | adds `Set-Cookie` |
| `ctx.set_header(name, value)` / `ctx.set_status(code)` | response headers and status |
| `ctx.method()`, `ctx.path()`, `ctx.remote_addr()`, `ctx.elapsed()` | request info |
| `ctx.revalidate()` / `ctx.revalidate_path("/blog/x")` / `ctx.revalidate_prefix("/blog")` | purges cached pages and client route data, see [revalidation](#revalidation) |

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
| `Error::validation([(field, msg)])` | 422 with `fieldErrors` |
| `not_found()` | 404, renders `not-found.tsx` |
| `Error::conflict(msg)` | 409 |
| `redirect("/x")` | 303 (`Error::permanent_redirect` for 308) |
| `?` on any `std::error::Error` | 500 |
