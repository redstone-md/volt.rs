export const revalidate = 2;

export default function Time() {
  return <p id="time">{Date.now()}</p>;
}
