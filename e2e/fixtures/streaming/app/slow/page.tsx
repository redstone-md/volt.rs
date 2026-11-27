import type { PageProps } from "@volt/api";

export default function Slow({ data }: PageProps<"/slow">) {
  return <h1 id="slow-title">{data.title}</h1>;
}
