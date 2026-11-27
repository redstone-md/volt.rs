"use client";

import { useState } from "react";
import type { PageProps } from "volt-rs";
import Link from "volt-rs/link";

export default function Item({ params }: PageProps<undefined, { id: string }>) {
  const [count, setCount] = useState(0);
  return (
    <main>
      <h1 id="item">Item {params.id}</h1>
      <p id="count">{count}</p>
      <button id="inc" onClick={() => setCount((c) => c + 1)}>
        inc
      </button>
      <Link href={`/items/${Number(params.id) + 1}`}>Next item</Link>
    </main>
  );
}
