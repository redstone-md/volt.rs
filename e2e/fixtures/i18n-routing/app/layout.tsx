import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";
import { useLocale, usePathname } from "volt-rs/navigation";

export const metadata = {
  title: { default: "i18n", template: "%s · i18n" },
  alternates: { languages: { fr: "https://fr.example.com/" } },
};

export default function Root({ children }: LayoutProps) {
  const locale = useLocale();
  const pathname = usePathname();
  return (
    <html lang={locale}>
      <body>
        <nav>
          <Link href="/" id="home">
            Home
          </Link>{" "}
          <Link href="/about" id="about">
            About
          </Link>{" "}
          <Link href="/blog/hello" id="post">
            Post
          </Link>{" "}
          <Link href={pathname} locale="de" id="to-de">
            Deutsch
          </Link>{" "}
          <Link href={pathname} locale="en" id="to-en">
            English
          </Link>{" "}
          <Link href="/about" locale={false} id="raw">
            Raw
          </Link>
        </nav>
        {children}
      </body>
    </html>
  );
}
