import type { ComponentType } from "react";

/** Props passed to the `loading` component while the dynamic component loads. */
export interface DynamicLoadingProps {
  isLoading?: boolean;
  pastDelay?: boolean;
  error?: Error | null;
}

/**
 * Lazily loaded component, compatible with `next/dynamic`. Server-rendered and streamed once loaded,
 * unless `ssr` is `false`: then it loads in the browser after hydration and `loading` shows until then.
 */
export default function dynamic<P = {}>(
  loader: () => Promise<{ default: ComponentType<P> } | ComponentType<P>>,
  options?: { loading?: ComponentType<DynamicLoadingProps>; ssr?: boolean },
): ComponentType<P>;
