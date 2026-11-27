import type { PageProps } from "@volt/api";
import { useLocale } from "volt-rs/navigation";

export const metadata = { title: "About" };

export default function About({ data }: PageProps<"/about">) {
  return (
    <main>
      <h1>{data.greeting}</h1>
      <p id="loader-locale">{data.locale}</p>
      <p id="locale">{useLocale()}</p>
    </main>
  );
}
