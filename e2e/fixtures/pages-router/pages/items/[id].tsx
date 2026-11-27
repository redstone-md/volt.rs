import type { GetStaticPaths, GetStaticProps } from "next";

type Props = { id: string; renderedAt: number };

export const getStaticPaths: GetStaticPaths = async () => ({ paths: [], fallback: "blocking" });

export const getStaticProps: GetStaticProps<Props> = async ({ params }) => ({
  props: { id: String(params.id), renderedAt: Date.now() + Math.random() },
});

export default function Item({ id, renderedAt }: Props) {
  return (
    <article>
      <h1>Item {id}</h1>
      <p data-testid="rendered-at">{renderedAt}</p>
    </article>
  );
}
