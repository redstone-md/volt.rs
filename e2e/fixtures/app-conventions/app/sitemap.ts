import type { MetadataRoute } from "next";

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  return [
    { url: "https://conventions.test/", lastModified: new Date("2024-01-02T03:04:05Z"), changeFrequency: "weekly", priority: 1 },
    { url: "/blog/hello", changeFrequency: "monthly", priority: 0.5 },
  ];
}
