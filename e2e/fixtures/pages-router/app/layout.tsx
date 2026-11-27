import type { LayoutProps } from "volt-rs";

export const metadata = { title: { default: "app router", template: "%s · app router" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
