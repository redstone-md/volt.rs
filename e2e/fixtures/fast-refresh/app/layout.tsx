import type { LayoutProps } from "volt-rs";
import "./globals.css";

export const metadata = { title: "fast-refresh" };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
