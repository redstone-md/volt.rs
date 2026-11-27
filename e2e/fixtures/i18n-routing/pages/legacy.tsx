import type { GetServerSideProps } from "next";
import { useRouter } from "next/router";

type Props = { serverLocale: string };

export const getServerSideProps: GetServerSideProps<Props> = async ({ locale }) => ({
  props: { serverLocale: locale ?? "none" },
});

export default function Legacy({ serverLocale }: Props) {
  const router = useRouter();
  return (
    <main>
      <h1>Legacy</h1>
      <p id="server-locale">{serverLocale}</p>
      <p id="router-locale">
        {router.locale}|{router.locales?.join(",")}|{router.defaultLocale}
      </p>
    </main>
  );
}
