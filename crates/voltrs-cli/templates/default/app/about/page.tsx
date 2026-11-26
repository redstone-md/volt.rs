import type { Metadata } from "volt-rs";

export const metadata: Metadata = {
  title: "About",
  description: "How this app is built.",
};

export default function About() {
  return (
    <article className="space-y-4">
      <h1 className="text-3xl font-bold tracking-tight">About</h1>
      <p className="text-muted-foreground">
        This page has no loader, so <code>volt build</code> prerenders it to static HTML. Pages are React
        components in <code>app/</code>; loaders, actions and API routes are Rust functions in{" "}
        <code>server/main.rs</code>.
      </p>
    </article>
  );
}
