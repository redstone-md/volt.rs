import type { PageProps } from "@volt/api";

export const metadata = { title: "Dashboard" };

export default function Dashboard({ data }: PageProps<"/dashboard">) {
  return (
    <main>
      <h1>Dashboard</h1>
      <p id="user">{data.user ?? "nobody"}</p>
    </main>
  );
}
