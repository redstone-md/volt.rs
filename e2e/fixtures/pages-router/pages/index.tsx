import Head from "next/head";
import Link from "next/link";
import { useRouter } from "next/router";

export default function Home() {
  const router = useRouter();
  return (
    <main>
      <Head>
        <title>Home | pages</title>
      </Head>
      <h1>Pages home</h1>
      <p data-testid="pathname">{router.pathname}</p>
      <Link href="/modern">App Router page</Link>
      <button type="button" onClick={() => router.push({ pathname: "/blog/[slug]", query: { slug: "pushed", ref: "button" } })}>
        Push
      </button>
    </main>
  );
}
