import type { PageProps } from "@volt/api";

export function generateMetadata({ data }: PageProps<"/photos/[id]">) {
  return { title: data.title };
}

export default function PhotoPage({ data }: PageProps<"/photos/[id]">) {
  return (
    <main>
      <h1 id="photo-page">{data.title}</h1>
      <p id="photo-source">{data.source}</p>
    </main>
  );
}
