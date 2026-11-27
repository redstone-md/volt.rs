import Link from "volt-rs/link";
import { Counter } from "./counter";
import { helper } from "./helper";
import { LABEL } from "./label";

export default function Home() {
  return (
    <main>
      <h1 id="title">Home</h1>
      <p id="label">{LABEL}</p>
      <p id="helper">{helper()}</p>
      <Counter />
      <Link href="/other">Other</Link>
    </main>
  );
}
