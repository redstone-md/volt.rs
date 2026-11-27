import type { LayoutProps } from "volt-rs";
import Link from "volt-rs/link";

export default function Shop({ children, side }: LayoutProps<{}, "side">) {
  return (
    <div>
      <Link href="/shop/a" id="to-shop-a">
        A
      </Link>{" "}
      <Link href="/shop/b" id="to-shop-b">
        B
      </Link>
      <section id="shop-children">{children}</section>
      <aside id="shop-side">{side}</aside>
    </div>
  );
}
