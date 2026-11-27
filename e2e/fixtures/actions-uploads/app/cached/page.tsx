import type { PageProps } from "@volt/api";

export const metadata = { title: "Cached" };
export const revalidate = 3600;

export default function CachedPage({ data }: PageProps<"/cached">) {
  return (
    <main>
      <h1>Cached</h1>
      <p id="renders">{data.renders}</p>
    </main>
  );
}
