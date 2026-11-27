import Link from "volt-rs/link";
import type { PageProps } from "volt-rs";

export const dynamicParams = false;

export function generateStaticParams() {
  return [{ slug: "hello" }, { slug: "world" }];
}

export async function generateMetadata({ params }: PageProps<undefined, { slug: string }>) {
  const { slug } = await params;
  return { title: `Post ${slug}` };
}

export default function Post({ params }: PageProps<undefined, { slug: string }>) {
  return (
    <article>
      <h1>Post {params.slug}</h1>
      <p id="rendered">{Date.now()}</p>
      <Link href="/">Home</Link>
    </article>
  );
}
