import Image from "next/image";
import hero from "./assets/hero.png";
import { Loaded } from "./loaded";

const cdn = ({ src, width }: { src: string; width: number }) => `https://cdn.test${src}?w=${width}`;

export default function Home() {
  return (
    <main>
      <Image id="fixed" src="/photo.jpg" alt="Fixed" width={400} height={300} />
      <Image id="responsive" src="/photo.jpg" alt="Responsive" width={1200} height={800} sizes="50vw" />
      <Image id="priority" src="/graphic.png" alt="Priority" width={600} height={400} priority />
      <Image id="hero" src={hero} alt="Hero" placeholder="blur" />
      <Image
        id="dataurl"
        src="/graphic.png"
        alt="Data URL"
        width={60}
        height={40}
        placeholder="data:image/gif;base64,R0lGODlhAQABAAAAACw="
      />
      <Image id="quality" src="/photo.jpg" alt="Quality" width={64} height={48} quality={40} />
      <Image id="rounded" src="/photo.jpg" alt="Rounded" width={64} height={48} quality={55} />
      <Image id="override" src="/photo.jpg" alt="Override" width={64} height={48} overrideSrc="/photo.jpg" />
      <Image id="unoptimized" src="/photo.jpg" alt="Unoptimized" width={64} height={48} unoptimized />
      <Image id="custom" src="/photo.jpg" alt="Custom" width={64} height={48} loader={cdn} />
      <Image id="svg" src="/logo.svg" alt="Logo" width={10} height={10} />
      <Image id="remote-allowed" src="http://127.0.0.1:9/allowed/pic.png" alt="Remote" width={64} height={64} />
      <Image id="remote-blocked" src="http://127.0.0.1:9/blocked/pic.png" alt="Blocked" width={64} height={64} />
      <div id="frame" style={{ position: "relative", width: 300, height: 200 }}>
        <Image id="fill" src="/graphic.png" alt="Fill" fill />
      </div>
      <Loaded />
    </main>
  );
}
