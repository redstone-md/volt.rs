import type { PageProps } from "volt-rs";

export async function getStaticPaths() {
  return { paths: [{ params: { id: "1" } }, "/posts/2"], fallback: false };
}

export default function Post({ params }: PageProps<undefined, { id: string }>) {
  return <h1>Pages post {params.id}</h1>;
}
