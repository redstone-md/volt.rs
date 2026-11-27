"use client";
import { useState } from "react";

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
