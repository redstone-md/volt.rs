import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Counter } from "@/components/counter";

export const metadata = { title: "Home" };

export default function Home() {
  return (
    <div className="space-y-8">
      <section className="space-y-2">
        <h1 className="text-3xl font-bold tracking-tight">React, served by Rust</h1>
        <p className="text-muted-foreground">
          This page was rendered on the server and hydrated in the browser. Add Rust loaders and server actions
          later by creating <code>Cargo.toml</code> and <code>server/main.rs</code>.
        </p>
      </section>
      <Card>
        <CardHeader>
          <CardTitle>Counter</CardTitle>
          <CardDescription>Client state after hydration.</CardDescription>
        </CardHeader>
        <CardContent>
          <Counter />
        </CardContent>
      </Card>
    </div>
  );
}
