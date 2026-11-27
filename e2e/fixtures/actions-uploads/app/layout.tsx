import type { LayoutProps } from "volt-rs";

export const metadata = { title: { default: "actions-uploads", template: "%s · actions-uploads" } };

export default function Root({ children }: LayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
