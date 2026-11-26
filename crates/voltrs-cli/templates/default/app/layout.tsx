import type { LayoutProps, Metadata } from "volt-rs";
import Link from "volt-rs/link";

import "./globals.css";

export const metadata: Metadata = {
  title: { default: "{{name}}", template: "%s · {{name}}" },
  description: "A volt.rs app: React on a Rust server.",
  icons: "/favicon.svg",
  openGraph: { type: "website", siteName: "{{name}}" },
};

export default function RootLayout({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body className="min-h-screen">
        <header className="border-b">
          <nav className="mx-auto flex h-14 max-w-3xl items-center gap-6 px-6 text-sm">
            <Link href="/" className="font-semibold">
              ⚡ {"{{name}}"}
            </Link>
            <Link href="/about" className="text-muted-foreground hover:text-foreground">
              About
            </Link>
          </nav>
        </header>
        <main className="mx-auto max-w-3xl px-6 py-12">{children}</main>
      </body>
    </html>
  );
}
