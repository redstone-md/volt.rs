import type { LayoutProps } from "volt-rs";

export const metadata = { title: { default: "static-generation", template: "%s · static-generation" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
