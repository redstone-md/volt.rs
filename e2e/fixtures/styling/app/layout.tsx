import type { LayoutProps } from "volt-rs";
import localFont from "next/font/local";
import "./globals.scss";

const brand = localFont({
  src: [{ path: "./fonts/Geist.woff2", weight: "400", style: "normal" }],
  variable: "--font-brand",
  fallback: ["Helvetica Neue", "sans-serif"],
});

export const metadata = { title: "styling" };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en" className={brand.variable}>
      <body className={brand.className}>{children}</body>
    </html>
  );
}
