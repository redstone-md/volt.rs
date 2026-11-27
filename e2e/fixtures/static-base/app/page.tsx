import Link from "volt-rs/link";

export default function Home() {
  return (
    <main>
      <h1>Home</h1>
      <Link href="/about">About</Link>
      <Link href="/posts/2">Post 2</Link>
    </main>
  );
}
