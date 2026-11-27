"use client";

import type { ErrorProps } from "volt-rs";

export default function GlobalError({ error, reset }: ErrorProps) {
  return (
    <html lang="en">
      <body>
        <h1 id="global-error">Global error</h1>
        <p id="global-message">{error?.message}</p>
        <button id="global-reset" onClick={reset}>
          Try again
        </button>
      </body>
    </html>
  );
}
