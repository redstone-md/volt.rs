import type { PageProps } from "@volt/api";
import Link from "volt-rs/link";

export const metadata = { title: "Me" };

export default function Me({ data }: PageProps<"/me">) {
  return (
    <main>
      <h1>Me</h1>
      <p id="user">{data.user ?? "anonymous"}</p>
      <Link href="/items">Items</Link>
    </main>
  );
}
