import { useEffect } from "react";
import type { AppProps } from "next/app";
import Link from "next/link";

declare global {
  interface Window {
    __routerEvents: string[];
  }
}

export default function App({ Component, pageProps, router }: AppProps) {
  useEffect(() => {
    window.__routerEvents = [];
    const record = (name: string) => (url: string) => window.__routerEvents.push(`${name} ${url}`);
    const handlers = {
      routeChangeStart: record("start"),
      beforeHistoryChange: record("history"),
      routeChangeComplete: record("complete"),
    };
    for (const [name, handler] of Object.entries(handlers)) router.events.on(name as "routeChangeStart", handler);
    return () => {
      for (const [name, handler] of Object.entries(handlers)) router.events.off(name as "routeChangeStart", handler);
    };
  }, [router.events]);

  return (
    <div data-testid="app-shell">
      <nav>
        <Link href="/">Home</Link> <Link href="/blog/hello-world">Blog</Link> <Link href="/profile">Profile</Link>{" "}
        <Link href="/static">Static</Link>
      </nav>
      <Component {...pageProps} />
    </div>
  );
}
