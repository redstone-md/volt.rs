import type { LayoutProps } from "volt-rs";

export default function Err({ children, side, boom }: LayoutProps<{}, "side" | "boom">) {
  return (
    <div>
      <section id="err-children">{children}</section>
      <aside id="err-side">{side}</aside>
      <aside id="err-boom">{boom}</aside>
    </div>
  );
}
