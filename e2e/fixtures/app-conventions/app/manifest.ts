import type { MetadataRoute } from "next";

export default function manifest(): MetadataRoute.Manifest {
  return { name: "Conventions", short_name: "Conv", start_url: "/", display: "standalone", theme_color: "#000000" };
}
