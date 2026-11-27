import { useState } from "react";
import { echo } from "@volt/api";
import { useI18n, useRouter } from "volt-rs/navigation";

export default function Home() {
  const { locale, locales, defaultLocale } = useI18n();
  const router = useRouter();
  const [echoed, setEchoed] = useState("");
  const send = async () => {
    const result = await echo({ text: "hi" });
    setEchoed(`${result.locale}:${result.text}`);
  };
  return (
    <main>
      <h1>{locale === "de" ? "Willkommen" : "Welcome"}</h1>
      <p id="locale">{locale}</p>
      <p id="locales">
        {locales.join(",")}|{defaultLocale}
      </p>
      <button type="button" id="echo" onClick={send}>
        Echo
      </button>
      <p id="echoed">{echoed}</p>
      <button type="button" id="push-de" onClick={() => router.push("/about", { locale: "de" })}>
        About (de)
      </button>
    </main>
  );
}
