import { Html, Head, Main, NextScript } from "next/document";

export default function Document() {
  return (
    <Html lang="fr" data-theme="dark">
      <Head>
        <meta name="generator" content="pages-router-fixture" />
      </Head>
      <body className="pages-body">
        <Main />
        <NextScript />
      </body>
    </Html>
  );
}
