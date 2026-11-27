import type { PageProps } from "volt-rs";

export const dynamicParams = false;

export function generateStaticParams() {
  return [1, 2, 3].map((id) => ({ id: String(id) }));
}

export default function Post({ params }: PageProps<undefined, { id: string }>) {
  return <h1>Post {params.id}</h1>;
}
