import Link from "volt-rs/link";

export default function Home() {
  return (
    <main>
      <h1>Home</h1>
      <Link href="/blog/hello">Hello post</Link>
      <Link href="/blog/world">World post</Link>
      <Link href="/lang/fr/about">About (fr)</Link>
    </main>
  );
}
