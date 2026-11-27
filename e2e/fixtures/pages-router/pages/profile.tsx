import Head from "next/head";
import type { GetServerSideProps } from "next";

type Props = { name: string; language: string; greeting: string };

export const getServerSideProps: GetServerSideProps = async () => ({ props: { greeting: "Bonjour" } });

export default function Profile({ name, language, greeting }: Props) {
  return (
    <section>
      <Head>
        <title>Profile | pages</title>
      </Head>
      <h1>Profile</h1>
      <p data-testid="loader">
        {name} writes {language}
      </p>
      <p data-testid="greeting">{greeting}</p>
    </section>
  );
}
