import type { PageProps } from "@volt/api";
import Link from "volt-rs/link";
import { useParams, useRouter } from "volt-rs/navigation";

export default function PhotoModal({ data }: PageProps<"/@modal/(.)photos/[id]">) {
  const router = useRouter();
  const { id } = useParams<{ id: string }>();
  const next = (data.id % 6) + 1;
  return (
    <div role="dialog" id="modal">
      <p id="modal-title">{data.title}</p>
      <p id="modal-source">{data.source}</p>
      <p id="modal-param">{id}</p>
      <Link href={`/photos/${next}`} id="modal-next">
        Next
      </Link>
      <button type="button" id="modal-close" onClick={() => router.back()}>
        Close
      </button>
      <Link href="/dashboard" id="modal-home">
        Dashboard
      </Link>
    </div>
  );
}
