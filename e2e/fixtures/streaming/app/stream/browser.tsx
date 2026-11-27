export default function Browser() {
  return <p id="browser">Rendered in {typeof window === "undefined" ? "the server" : "the browser"}</p>;
}
