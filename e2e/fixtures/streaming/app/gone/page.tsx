import { notFound } from "volt-rs/navigation";

export const dynamic = "force-dynamic";

export default function Gone(): never {
  notFound();
}
