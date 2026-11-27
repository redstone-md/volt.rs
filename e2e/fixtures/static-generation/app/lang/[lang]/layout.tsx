import type { LayoutProps } from "volt-rs";

export function generateStaticParams() {
  return [{ lang: "en" }, { lang: "fr" }];
}

export default function Lang({ children }: LayoutProps) {
  return <section>{children}</section>;
}
