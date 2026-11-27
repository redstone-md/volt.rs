import type { MetadataRoute } from "next";

export const revalidate = 3600;

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  return [
    { url: "https://conventions.test/", lastModified: new Date("2024-01-02T03:04:05Z"), changeFrequency: "weekly", priority: 1 },
    { url: "/blog/hello", changeFrequency: "monthly", priority: 0.5 },
    { url: `/render/${Math.random().toString(36).slice(2)}` },
  ];
}
