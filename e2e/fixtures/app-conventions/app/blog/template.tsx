"use client";

import { type ReactNode, useEffect } from "react";

export default function BlogTemplate({ children }: { children: ReactNode }) {
  useEffect(() => {
    const w = window as unknown as { blogTemplateMounts?: number };
    w.blogTemplateMounts = (w.blogTemplateMounts ?? 0) + 1;
  }, []);
  return <div id="blog-template">{children}</div>;
}
