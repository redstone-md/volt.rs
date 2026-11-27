import type { PageProps } from "volt-rs";

export async function generateStaticParams() {
  return [{ path: ["guide", "intro"] }];
}

export default function Doc({ params }: PageProps<undefined, { path: string[] }>) {
  return <h1>Doc {params.path.join("/")}</h1>;
}
