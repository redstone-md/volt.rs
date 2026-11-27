import { useActionState as useReactActionState } from "react";
import { greet } from "@volt/api";
import { useActionState } from "volt-rs/actions";

export const metadata = { title: "State" };

function ReactForm() {
  const [state, formAction] = useReactActionState(greet, null);
  return (
    <form action={formAction} id="react-form">
      <input name="name" defaultValue="React" />
      <button type="submit">Greet with React</button>
      <p id="react-greeting">{state ? state.message : "none"}</p>
    </form>
  );
}

function PermalinkForm() {
  const [state, formAction] = useActionState(greet, null, "/state?via=permalink");
  return (
    <form action={formAction} id="permalink-form">
      <input name="name" defaultValue="Permalink" />
      <button type="submit">Greet via permalink</button>
      <p id="permalink-greeting">{state ? state.message : "none"}</p>
    </form>
  );
}

export default function State() {
  const [state, formAction, pending, error] = useActionState(greet, null);
  const fields = error?.fieldErrors;
  return (
    <main>
      <h1>State</h1>
      <form action={formAction} id="volt-form">
        <input name="name" />
        {fields?.name && <p id="name-error">{fields.name[0]}</p>}
        <input name="age" />
        {fields?.age && <p id="age-error">{fields.age[0]}</p>}
        <button type="submit">{pending ? "Sending" : "Send"}</button>
      </form>
      <p id="greeting">{state ? `${state.message} #${state.count}` : "none"}</p>
      <p id="status">{error ? `${error.status}` : "ok"}</p>
      <ReactForm />
      <PermalinkForm />
    </main>
  );
}
