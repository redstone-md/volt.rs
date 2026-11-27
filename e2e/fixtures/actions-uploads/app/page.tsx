import { useState } from "react";
import { useFormStatus } from "react-dom";
import { upload, type PageProps } from "@volt/api";
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
      <p id="last">
        {last ? `${last.title}|${last.filename}|${last.size}|${last.contentType}|${last.extras}` : "none"}
      </p>
    </main>
  );
}
