import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";

export const metadata = { title: { default: "parallel-routes", template: "%s · parallel-routes" } };

export default function Root({ children, modal }: LayoutProps<{}, "modal">) {
  return (
    <html lang="en">
      <body>
        <nav>
          <Link href="/">Home</Link> <Link href="/photos">Photos</Link> <Link href="/dashboard">Dashboard</Link>{" "}
          <Link href="/strict">Strict</Link> <Link href="/strict/extra">Strict extra</Link>
        </nav>
        {children}
        {modal}
      </body>
    </html>
  );
}
