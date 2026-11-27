import type { LayoutProps } from "volt-rs";

export const metadata = { title: { default: "static-base", template: "%s · static-base" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
