import Head from "next/head";
import { useRouter } from "next/router";
import type { GetServerSideProps } from "next";

type Props = { shout: string; resolvedUrl: string; origin: string };

export const getServerSideProps: GetServerSideProps<Props> = async ({ params, resolvedUrl }) => {
  if (params.slug === "old") return { redirect: { destination: "/blog/new", permanent: false } };
  if (params.slug === "missing") return { notFound: true };
  const origin = "server-only-marker";
  return { props: { shout: String(params.slug).toUpperCase(), resolvedUrl, origin } };
};

export default function Post({ shout, resolvedUrl }: Props) {
  const { query, pathname, asPath } = useRouter();
  return (
    <article>
      <Head>
        <title>{`Post ${query.slug}`}</title>
      </Head>
      <h1>Post {query.slug}</h1>
      <p data-testid="shout">{shout}</p>
      <p data-testid="pattern">{pathname}</p>
      <p data-testid="as-path">{asPath}</p>
      <p data-testid="resolved">{resolvedUrl}</p>
      <p data-testid="ref">{String(query.ref ?? "")}</p>
    </article>
  );
}
