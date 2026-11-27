import type { AnchorHTMLAttributes, ForwardRefExoticComponent, RefAttributes } from "react";

export type Href = string | { pathname?: string; query?: Record<string, string>; hash?: string };

export interface LinkProps extends Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  href: Href;
  replace?: boolean;
  scroll?: boolean;
  prefetch?: boolean;
  /** `[i18n]` locale to link to; `false` uses `href` as given. Defaults to the current locale. */
  locale?: string | false;
}

declare const Link: ForwardRefExoticComponent<LinkProps & RefAttributes<HTMLAnchorElement>>;
export default Link;
export { Link };
