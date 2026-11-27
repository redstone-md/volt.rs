import type { PageProps } from "volt-rs";

export const metadata = { title: "About" };

export default function About({ searchParams }: PageProps) {
  const { from } = searchParams as Record<string, string | undefined>;
  return (
    <main>
      <h1>About</h1>
      <p id="from">{from ?? "direct"}</p>
    </main>
  );
}
