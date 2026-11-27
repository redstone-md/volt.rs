import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";

export default function Strict({ children, side }: LayoutProps<{}, "side">) {
  return (
    <div>
      <Link href="/strict/other" id="to-other">
        Other
      </Link>
      <section id="strict-children">{children}</section>
      <aside id="side">{side}</aside>
    </div>
  );
}
