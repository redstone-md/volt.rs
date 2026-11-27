"use client";

import type { ErrorProps } from "volt-rs";

export default function ErrorPage({ error }: ErrorProps) {
  return <h1 id="root-error">Root error: {error?.message}</h1>;
}
