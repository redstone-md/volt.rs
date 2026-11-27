import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";

export default function Dashboard({ children, analytics, team }: LayoutProps<{}, "analytics" | "team">) {
  return (
    <div>
      <nav>
        <Link href="/dashboard" id="to-overview">
          Overview
        </Link>{" "}
        <Link href="/dashboard/settings" id="to-settings">
          Settings
        </Link>{" "}
        <Link href="/dashboard/views" id="to-views">
          Views
        </Link>
      </nav>
      <section id="children">{children}</section>
      <section id="analytics">{analytics}</section>
      <section id="team">{team}</section>
    </div>
  );
}
