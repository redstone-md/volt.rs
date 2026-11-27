import type { PageProps } from "volt-rs";
import { unstable_noStore } from "next/cache";

export const dynamic = "force-static";
export const fetchCache = "force-no-store";
export const runtime = "nodejs";
export const preferredRegion = "auto";

export default function Dynamic({ searchParams }: PageProps) {
  unstable_noStore();
  return <h1 id="search">q={String(searchParams.q ?? "")}</h1>;
}
