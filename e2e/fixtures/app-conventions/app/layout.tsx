"use client";

import { useEffect, useState } from "react";
import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";
import {
  usePathname,
  useSelectedLayoutSegment,
  useSelectedLayoutSegments,
  useServerInsertedHTML,
} from "next/navigation";

export const metadata = { title: { default: "Conventions", template: "%s · Conventions" } };

function Crash({ when }: { when: boolean }) {
  if (when) throw new Error("the root layout crashed");
  return null;
}

export default function Root({ children }: LayoutProps) {
  const segment = useSelectedLayoutSegment();
  const segments = useSelectedLayoutSegments();
  const pathname = usePathname();
  const [crash, setCrash] = useState(false);
  useServerInsertedHTML(() => <meta name="inserted" content={`server ${pathname}`} />);
  useEffect(() => {
    const w = window as unknown as { rootMounts?: number };
    w.rootMounts = (w.rootMounts ?? 0) + 1;
  }, []);
  return (
    <html lang="en">
      <body>
        <nav>
          <Link href="/">Home</Link> <Link href="/blog">Blog</Link> <Link href="/blog/hello">Hello</Link>{" "}
          <Link href="/blog/world">World</Link> <Link href="/blog/missing">Missing</Link>{" "}
          <Link href="/cart">Cart</Link>
        </nav>
        <p id="root-segment">{String(segment)}</p>
        <p id="root-segments">{segments.join("|")}</p>
        <button id="crash" onClick={() => setCrash(true)}>
          Crash
        </button>
        <Crash when={crash || pathname === "/layout-crash"} />
        {children}
      </body>
    </html>
  );
}
