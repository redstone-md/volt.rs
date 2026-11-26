import type { ComponentType } from "react";

export default function dynamic<P = {}>(
  loader: () => Promise<{ default: ComponentType<P> } | ComponentType<P>>,
  options?: { loading?: ComponentType; ssr?: boolean },
): ComponentType<P>;
