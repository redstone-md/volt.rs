import type { PageProps } from "volt-rs";
import { notFound } from "next/navigation";

export function generateMetadata({ params }: PageProps<undefined, { slug: string }>) {
  return { title: params.slug };
}

export default function Post({ params }: PageProps<undefined, { slug: string }>) {
  if (params.slug === "missing") notFound();
  return <h1 id="post">Post {params.slug}</h1>;
}
