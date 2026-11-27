import Head from "next/head";
import type { GetStaticProps } from "next";

type Props = { message: string; renderedAt: number };

export const getStaticProps: GetStaticProps<Props> = async () => ({
  props: { message: "built ahead of time", renderedAt: Date.now() },
  revalidate: 3600,
});

export default function Static({ message, renderedAt }: Props) {
  return (
    <section>
      <Head>
        <title>Static | pages</title>
      </Head>
      <h1>Static</h1>
      <p data-testid="message">{message}</p>
      <p data-testid="rendered-at">{renderedAt}</p>
    </section>
  );
}
