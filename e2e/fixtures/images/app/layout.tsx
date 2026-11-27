import type { LayoutProps } from "volt-rs";

export const metadata = { title: "images" };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
