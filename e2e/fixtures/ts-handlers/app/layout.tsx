import type { LayoutProps } from "volt-rs";
import Link from "next/link";

export const metadata = { title: { default: "ts-handlers", template: "%s · ts-handlers" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>
        <nav>
          <Link href="/">Home</Link> <Link href="/dashboard">Dashboard</Link> <Link href="/rewritten">Rewritten</Link>{" "}
          <Link href="/old-home">Old home</Link>
        </nav>
        {children}
      </body>
    </html>
  );
}
