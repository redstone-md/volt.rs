import type { PageProps } from "volt-rs";

export default function About({ params }: PageProps<undefined, { lang: string }>) {
  return <h1>About {params.lang}</h1>;
}
