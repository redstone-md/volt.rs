import type { PageProps } from "volt-rs";

export default function Doc({ params }: PageProps<undefined, { slug: string[] }>) {
  return <h1 id="doc">Doc {params.slug.join("/")}</h1>;
}
