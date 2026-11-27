import { Suspense, use } from "react";
import lazyComponent from "volt-rs/dynamic";

export const dynamic = "force-dynamic";
export const metadata = { title: "Stream" };

const Widget = lazyComponent(() => import("./widget"), { loading: () => <p id="widget-loading">Widget loading</p> });
const Browser = lazyComponent(() => import("./browser"), {
  ssr: false,
  loading: () => <p id="browser-loading">Browser only loading</p>,
});

const later = (ms: number, value: string) => new Promise<string>((resolve) => setTimeout(() => resolve(value), ms));

function Late({ value }: { value: Promise<string> }) {
  return <p id="late">{use(value)}</p>;
}

export default function StreamPage() {
  return (
    <>
      <h1>Stream</h1>
      <Suspense fallback={<p id="fallback">Waiting for data</p>}>
        <Late value={later(1200, "Late content")} />
      </Suspense>
      <Widget />
      <Browser />
    </>
  );
}
