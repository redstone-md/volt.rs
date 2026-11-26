import type { ScriptHTMLAttributes } from "react";

export interface ScriptProps extends ScriptHTMLAttributes<HTMLScriptElement> {
  strategy?: "beforeInteractive" | "afterInteractive" | "lazyOnload";
  onReady?: () => void;
}

export default function Script(props: ScriptProps): JSX.Element | null;
export { Script };
