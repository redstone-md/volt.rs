import type { PageProps } from "@volt/api";

export const dynamic = "force-static";

export default function WhoPage({ data }: PageProps<"/who">) {
  return (
    <main>
      <h1 id="who">{data.user ?? "anonymous"}</h1>
      <p id="who-query">{data.query ?? "none"}</p>
    </main>
  );
}
