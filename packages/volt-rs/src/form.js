import { createContext, createElement as h, forwardRef, useContext, useTransition } from "react";
import { endpoint } from "./internal/config.js";
import { getRouter } from "./internal/router.js";

const FormStatus = createContext({ pending: false });

/** Pending state of the closest `<Form>`. `useFormStatus` from `react-dom` works as well. */
export function useFormPending() {
  return useContext(FormStatus).pending;
}

/**
 * A form bound to a server action. Works without JavaScript (native multipart POST, so file inputs
 * upload too), and with JavaScript submits in place and refreshes the route data.
 */
const Form = forwardRef(function Form(
  { action, refresh = true, onSuccess, onError, onSubmit, encType, children, ...rest },
  ref,
) {
  const [pending, startTransition] = useTransition();
  const name = action?.$$volt;
  // URLs and plain React form actions (for example a `useActionState` dispatcher) render as-is.
  if (typeof action === "string" || (typeof action === "function" && !name)) {
    return h("form", { ...rest, ref, action, onSubmit, encType }, children);
  }
  if (!name) throw new Error("volt-rs: <Form action> expects an action from @volt/api, a function or a URL");
  const handleSubmit = (event) => {
    onSubmit?.(event);
    if (event.defaultPrevented) return;
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form, event.nativeEvent?.submitter);
    // Starting the transition inside the submit event lets react-dom's useFormStatus report it as pending.
    startTransition(async () => {
      try {
        const result = action.$$run ? await action.$$run(data) : { data: await action(data), revalidated: false };
        if (refresh && !result.revalidated) await getRouter()?.refresh();
        onSuccess?.(result.data, form);
      } catch (error) {
        if (onError) onError(error);
        else throw error;
      }
    });
  };
  return h(
    FormStatus.Provider,
    { value: { pending } },
    h(
      "form",
      { ...rest, ref, method: "post", encType: encType ?? "multipart/form-data", action: endpoint(`/_volt/action/${name}`), onSubmit: handleSubmit, "aria-busy": pending || undefined },
      children,
    ),
  );
});

export default Form;
export { Form };
