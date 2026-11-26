"use client";

import { useState } from "react";
import { Minus, Plus } from "lucide-react";

import { Button } from "@/components/ui/button";

export function Counter() {
  const [count, setCount] = useState(0);
  return (
    <div className="flex items-center gap-4">
      <Button variant="outline" size="icon" aria-label="Decrement" onClick={() => setCount((c) => c - 1)}>
        <Minus />
      </Button>
      <span className="w-12 text-center text-2xl font-semibold tabular-nums">{count}</span>
      <Button variant="outline" size="icon" aria-label="Increment" onClick={() => setCount((c) => c + 1)}>
        <Plus />
      </Button>
    </div>
  );
}
