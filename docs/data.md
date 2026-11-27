# Data: loaders, server actions, API routes and route handlers

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

## Route handlers

Next.js route handlers work unchanged for code that does not need Node.js: an `app/**/route.ts` (or `.js`) exports functions named after HTTP methods, which take a Web `Request` and return a Web `Response`. They run in the same QuickJS workers that render React.

```ts
// app/api/posts/[id]/route.ts
import { type NextRequest, NextResponse } from "next/server";

export async function GET(request: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const draft = request.nextUrl.searchParams.get("draft") === "1";
  return NextResponse.json({ id, draft });
}

export async function POST(request: Request) {
  const body = await request.json();
  const response = NextResponse.json({ saved: body }, { status: 201 });
  response.cookies.set("last-post", String(body.id), { httpOnly: true });
  return response;
}
```

- `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE` and `OPTIONS` are supported. `HEAD` falls back to `GET`, `OPTIONS` answers 204 with an `Allow` header, and other methods get 405 with `Allow`.
- `params` is a promise that also carries the params as properties, so both `await params` (Next.js 15) and `params.id` (Next.js 14) work.
- Bodies can be strings, JSON (`Response.json`), `Uint8Array` / `ArrayBuffer` / `Blob` (binary, passed to Rust as bytes), `URLSearchParams`, `FormData` or a `ReadableStream`. A body known up front is sent with a `Content-Length` (and gzip when it is large enough); a `ReadableStream` body is forwarded chunk by chunk as the handler writes it, uncompressed, so server-sent events and AI token streams work. The 10-second render deadline only covers the time until the handler returns its `Response`: the stream then runs until it closes or the client disconnects, holding one of the worker's 8 interleaved jobs meanwhile (`volt dev` ends open streams when it reloads the bundle). Requests offer `text()`, `json()`, `arrayBuffer()`, `bytes()`, `blob()` and `formData()` (urlencoded and multipart).
- `redirect()` and `notFound()` from `next/navigation` turn into a 307/308 redirect and a 404.
- Matching order: Rust API routes first, then route handlers, then pages; a more specific page pattern wins over a handler's dynamic one. A folder cannot hold both `page.tsx` and `route.ts`, and a handler cannot answer the URL of a page in another route group: both fail the build.
- `export const dynamic = "force-static"` on a handler without params runs its `GET` once at `volt build` and serves the stored response (status, headers and body) from then on; `volt export` writes it as a file, e.g. `app/feed.xml/route.ts` becomes `feed.xml`. Other handlers run on every request, as in Next.js 15.
- `volt routes` lists the handlers next to the pages.

The QuickJS runtime provides `Request`, `Response` (`json()`, `redirect()`, `error()`), `Headers` (`getSetCookie()`), `URL`, `URLSearchParams`, `FormData`, `Blob`, `File`, `TextEncoder`, `TextDecoder`, `ReadableStream`, `AbortController`, `atob`/`btoa`, timers and `fetch`. There are no Node.js modules (`fs`, `crypto`, `Buffer`, ...): use a Rust API route for those. `next/server` (`volt-rs/server`) exports `NextRequest` (`nextUrl`, `cookies`), `NextResponse` (`json`, `redirect`, `rewrite`, `next`, `cookies`), `NextURL` and `userAgent`. Under `[build] base_path`, `request.nextUrl.pathname` leaves the base path out and `nextUrl.basePath` holds it, as in Next.js; `request.url`, `nextUrl.href` and a `clone()` with a new `pathname` keep it, so `NextResponse.redirect(url)` of a cloned `nextUrl` stays below the base path.

### fetch on the server

`fetch` in route handlers, middleware and server rendering (for example in `generateMetadata` or `app/sitemap.ts`) is backed by a Rust HTTP/1.1 client with rustls, verifying certificates with the platform's trust store. The worker keeps serving other requests while a response is on its way; redirects are followed (up to 20) unless `redirect: "manual"`, and response bodies are limited to 32 MB. Next.js caching options (`cache`, `next.revalidate`) are accepted and ignored: every call goes to the network.

```toml
[server]
fetch_allow = ["api.example.com", "*.cdn.example.com", "localhost:4000"]
fetch_timeout_secs = 10
```

With the default, empty `fetch_allow`, any host may be fetched as long as every address it resolves to is public, so code that fetches user-supplied URLs cannot reach `localhost`, private networks or cloud metadata endpoints. A non-empty list allows only the listed hosts (`*.` matches subdomains, `:port` pins the port, `*` allows any host) and lets those resolve to private addresses. `HTTP_PROXY` variables are not used.

## TypeScript middleware

`middleware.ts` (or `proxy.ts`, its Next.js 16 name) at the project root or in `src/` runs in QuickJS before routing, after Rust middleware:

```ts
// middleware.ts
import { type NextRequest, NextResponse } from "next/server";

export function middleware(request: NextRequest) {
  if (!request.cookies.get("session")) return NextResponse.redirect(new URL("/login", request.url));
  if (request.nextUrl.pathname === "/old") return NextResponse.rewrite(new URL("/new", request.url));
  const headers = new Headers(request.headers);
  headers.set("x-user", "ada");
  const response = NextResponse.next({ request: { headers } });
  response.headers.set("x-frame", "1");
  return response;
}

export const config = { matcher: ["/dashboard/:path*", "/((?!api|login).*)"] };
```

> **Middleware does not run for server actions.** Actions are posted to `/_volt/action/<name>`, which any client can call directly, so a middleware auth check never protects them (in Next.js too, actions must check auth themselves). Check the session inside every action, or in Rust middleware (`App::middleware`), which does run for them.

- `NextResponse.next()` continues; headers and cookies set on it are added to the final response, and `request.headers` replaces the headers that loaders, route handlers and API routes of that request see (`ctx.header("x-user")` in Rust).
- `NextResponse.rewrite(url)` serves another path of the app under the requested URL, query included; a URL on another origin is proxied (subject to `fetch_allow`).
- `NextResponse.redirect(url)` (307 by default) or any other `Response` answers the request directly.
- It runs for page and route handler requests, Rust API routes, `public/` files and SEO files. Client navigations apply it to the target path of `/_volt/data`: redirects become client-side navigations, rewrites load the rewritten route's data, and any other response makes the browser load the page as a document. Static assets, actions (see the warning above) and the image endpoint skip it.
- Before middleware runs, the path is brought to one spelling: duplicate slashes collapse, `.` and `..` segments (also percent-encoded ones) resolve, and each segment is percent-decoded and re-encoded once. The matcher, `request.nextUrl`, Rust middleware and routing all see that path, so `/dash%62oard` or `/x/../dashboard` cannot reach `/dashboard` past a matcher written for it. A `/_volt/data` target must start with `/`.
- `config.matcher` takes Next.js path patterns: `:name`, `:name*`, `:name+`, `:name?`, `:id(\\d+)` and `(a|b)` groups, as a string, an array, or `{ source, has, missing }` objects. `has` and `missing` take `{ type: "header" | "cookie" | "query", key, value? }` and `{ type: "host", value }` conditions: without `value` the header, cookie or query parameter must be present, otherwise `value` is a regular expression matching the whole value (or host name). The rule applies when every `has` condition holds and no `missing` one does. Like Next.js, the matcher must be written as literals; it is read at build time and matched in Rust, except patterns with lookarounds (`(?!...)`), which QuickJS evaluates. Without a matcher, middleware runs on every path. Patterns match the path without the base path and with the locale prefix.

## Ctx

Every loader, action and handler can take `ctx: Ctx` as its first argument.

| Method | Returns |
| --- | --- |
| `ctx.param("slug")` | route param |
| `ctx.param_as::<u32>("id")` | parsed param, 404 if invalid |
| `ctx.query::<T>()` / `ctx.query_param("q")` | query string |
| `ctx.header("user-agent")` / `ctx.headers()` | request headers |
| `ctx.cookie("session")` | cookie value |
| `ctx.locale()` / `ctx.locales()` | active [`[i18n]`](i18n.md) locale (from the URL prefix, or the calling page for actions) and the configured locales |
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

Middleware runs before routing for every request; returning `Some(response)` answers immediately. [TypeScript middleware](#typescript-middleware) runs after it.

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
