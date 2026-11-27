export const revalidate = 0;

export default function Now() {
  return <p id="time">{Date.now()}</p>;
}
