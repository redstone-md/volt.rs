import type { ErrorProps } from "volt-rs";

export default function BoomError({ error }: ErrorProps) {
  return <p>Boom failed: {error.message}</p>;
}
