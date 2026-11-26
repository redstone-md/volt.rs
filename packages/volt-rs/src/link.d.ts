import type { AnchorHTMLAttributes, ForwardRefExoticComponent, RefAttributes } from "react";

export type Href = string | { pathname?: string; query?: Record<string, string>; hash?: string };

export interface LinkProps extends Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  href: Href;
  replace?: boolean;
  scroll?: boolean;
  prefetch?: boolean;
}

declare const Link: ForwardRefExoticComponent<LinkProps & RefAttributes<HTMLAnchorElement>>;
export default Link;
export { Link };
