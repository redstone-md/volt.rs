import { addItem, login, type PageProps } from "@volt/api";
import Form from "volt-rs/form";
import Link from "volt-rs/link";

export const metadata = { title: "Items" };

export default function Items({ data }: PageProps<"/items">) {
  return (
    <main>
      <h1>Items</h1>
      <ul id="items">
        {data.items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
      <Form action={addItem} refresh={false} id="add">
        <input name="title" />
        <button type="submit">Add</button>
      </Form>
      <button id="add-code" type="button" onClick={() => addItem({ title: "from code" })}>
        Add from code
      </button>
      <Form action={login} refresh={false} id="login">
        <input name="name" defaultValue="ada" />
        <button type="submit">Login</button>
      </Form>
      <Link href="/me" id="me-link">
        Me
      </Link>
    </main>
  );
}
