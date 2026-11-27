import { useState } from "react";

export default function Hydrate() {
  const [count, setCount] = useState(0);
  return (
    <div id="__next">
      <h1>Legacy container</h1>
      <button type="button" onClick={() => setCount(count + 1)}>
        count {count}
      </button>
    </div>
  );
}
