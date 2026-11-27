import type { PageProps } from "volt-rs";
import { useLocale } from "volt-rs/navigation";

export function generateStaticParams() {
  return [{ slug: "hello" }];
}

export default function Post({ params }: PageProps<undefined, { slug: string }>) {
  return (
    <article>
      <h1>Post {params.slug}</h1>
      <p id="locale">{useLocale()}</p>
    </article>
  );
}
