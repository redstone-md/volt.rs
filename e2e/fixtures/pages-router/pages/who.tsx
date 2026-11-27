import type { GetStaticProps } from "next";

type Props = { user: string; label: string };

export const getStaticProps: GetStaticProps<Pick<Props, "label">> = async () => ({ props: { label: "static label" } });

export default function Who({ user, label }: Props) {
  return (
    <p data-testid="who">
      {user} {label}
    </p>
  );
}
