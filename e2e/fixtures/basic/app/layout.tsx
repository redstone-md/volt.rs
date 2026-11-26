import type { LayoutProps } from "volt-rs";

export const metadata = { title: { default: "basic", template: "%s · basic" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
