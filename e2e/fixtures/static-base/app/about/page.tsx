import Link from "volt-rs/link";
import { usePathname } from "volt-rs/navigation";

export const metadata = { title: "About" };

export default function About() {
  return (
    <main>
      <h1>About</h1>
      <p id="pathname">{usePathname()}</p>
      <Link href="/">Home</Link>
    </main>
  );
}
