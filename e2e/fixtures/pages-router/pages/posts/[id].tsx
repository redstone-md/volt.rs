import type { GetStaticPaths, GetStaticProps } from "next";

type Props = { title: string };

export const getStaticPaths: GetStaticPaths = async () => ({
  paths: [{ params: { id: "1" } }, { params: { id: "2" } }],
  fallback: false,
});

export const getStaticProps: GetStaticProps<Props> = async ({ params }) => ({
  props: { title: `Static post ${params.id}` },
});

export default function StaticPost({ title }: Props) {
  return <h1>{title}</h1>;
}
