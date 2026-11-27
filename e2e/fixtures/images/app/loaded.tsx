"use client";

import { useState } from "react";
import Image from "next/image";

export function Loaded() {
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  return (
    <section>
      <Image id="events" src="/graphic.png" alt="Events" width={64} height={43} onLoad={() => setLoaded(true)} />
      <Image id="broken" src="/missing.png" alt="Broken" width={64} height={43} onError={() => setFailed(true)} />
      <p id="loaded">{loaded ? "loaded" : "pending"}</p>
      <p id="failed">{failed ? "failed" : "ok"}</p>
    </section>
  );
}
