import { useState } from "react";
import { useFormStatus } from "react-dom";
import { tag, upload, type PageProps } from "@volt/api";
import Form, { useFormPending } from "volt-rs/form";

function Submit() {
  const { pending } = useFormStatus();
  const voltPending = useFormPending();
  return (
    <button type="submit" disabled={pending}>
      {pending ? "Uploading" : "Upload"}
      {voltPending ? " (volt)" : ""}
    </button>
  );
}

export default function Home({ data }: PageProps<"/">) {
  const [result, setResult] = useState("");
  const [tagged, setTagged] = useState("");
  const sendTagged = async () => {
    const file = new File(["abc"], "tags.txt", { type: "text/plain" });
    const r = await tag({ file, meta: { tags: ["x", "y"], rating: 4 } });
    const sent = "filename" in r.file ? r.file : null;
    setTagged(`${sent?.filename}|${sent?.size}|${r.meta.tags.join(",")}|${r.meta.rating}`);
  };
  const last = data.last;
  return (
    <main>
      <h1>Uploads</h1>
      <Form action={upload} onSuccess={(r) => setResult(`${r.filename}:${r.size}:${r.extras}`)}>
        <input name="title" defaultValue="Report" />
        <input type="file" name="file" />
        <input type="file" name="extras" multiple />
        <Submit />
      </Form>
      <p id="result">{result}</p>
      <button type="button" id="tag" onClick={sendTagged}>
        Tag
      </button>
      <p id="tagged">{tagged}</p>
      <p id="last">
        {last ? `${last.title}|${last.filename}|${last.size}|${last.contentType}|${last.extras}` : "none"}
      </p>
    </main>
  );
}
