import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";

export const metadata = { title: { default: "streaming", template: "%s · streaming" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>
        <nav>
          <Link href="/">Home</Link> <Link href="/stream">Stream</Link> <Link href="/slow" prefetch={false}>
            Slow
          </Link>{" "}
          <Link href="/quick">Quick</Link>
        </nav>
        <main>{children}</main>
      </body>
    </html>
  );
}
