import { Suspense, use, useEffect, useState } from "react";

export const dynamic = "force-dynamic";

// Never resolves on the server, so the engine aborts the render and the browser renders the boundary.
const pending =
  typeof window === "undefined" ? new Promise<string>(() => {}) : Promise.resolve("Rendered on the client");

function Never() {
  return <p id="never">{use(pending)}</p>;
}

function Mounted() {
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  return mounted ? <p id="mounted">Hydrated</p> : null;
}

export default function NeverPage() {
  return (
    <>
      <h1>Never</h1>
      <Suspense fallback={<p id="never-fallback">Waiting forever</p>}>
        <Never />
      </Suspense>
      <Mounted />
    </>
  );
}
