"use client";

import { useEffect } from "react";
import type { LayoutProps } from "volt-rs";
import { useSelectedLayoutSegment } from "volt-rs/navigation";

export const metadata = { title: "Blog", openGraph: { title: "Blog" } };

export default function BlogLayout({ children }: LayoutProps) {
  const segment = useSelectedLayoutSegment();
  useEffect(() => {
    const w = window as unknown as { blogLayoutMounts?: number };
    w.blogLayoutMounts = (w.blogLayoutMounts ?? 0) + 1;
  }, []);
  return (
    <section id="blog-layout">
      <p id="blog-segment">{String(segment)}</p>
      {children}
    </section>
  );
}
