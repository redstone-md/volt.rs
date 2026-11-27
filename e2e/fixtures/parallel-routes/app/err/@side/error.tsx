import type { ErrorProps } from "volt-rs";

export default function SideError({ error }: ErrorProps) {
  return <p>Side error {error.message}</p>;
}
