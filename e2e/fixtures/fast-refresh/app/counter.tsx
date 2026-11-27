"use client";
import { useState } from "react";

// Counts evaluations of this module, which updates of other modules must not repeat.
if (typeof window !== "undefined") window.__evaluations = (window.__evaluations ?? 0) + 1;

export function Counter() {
  const [count, setCount] = useState(0);
  const [text, setText] = useState("");
  return (
    <section>
      <button id="increment" onClick={() => setCount(count + 1)}>
        Count: {count}
      </button>
      <input id="text" value={text} onChange={(event) => setText(event.target.value)} />
      <p id="message">Version one</p>
    </section>
  );
}
