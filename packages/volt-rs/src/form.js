import { createContext, createElement as h, forwardRef, useContext, useTransition } from "react";
import { getRouter } from "./internal/router.js";

const FormStatus = createContext({ pending: false });

/** Pending state of the closest `<Form>`. */
export function useFormPending() {
  return useContext(FormStatus).pending;
}

/**
 * A form bound to a server action. Works without JavaScript (native POST),
 * and with JavaScript submits in place and refreshes the route data.
 */
const Form = forwardRef(function Form({ action, refresh = true, onSuccess, onError, onSubmit, children, ...rest }, ref) {
  const [pending, startTransition] = useTransition();
  const name = action?.$$volt;
  if (typeof action === "string") {
    return h("form", { ...rest, ref, action, onSubmit }, children);
  }
  if (!name) throw new Error("volt-rs: <Form action> expects an action from @volt/api or a URL");
  const handleSubmit = (event) => {
    onSubmit?.(event);
    if (event.defaultPrevented) return;
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form, event.nativeEvent?.submitter);
    startTransition(async () => {
      try {
        const result = await action(data);
        if (refresh) await getRouter()?.refresh();
        onSuccess?.(result, form);
      } catch (error) {
        if (onError) onError(error);
        else throw error;
      }
    });
  };
  return h(
    FormStatus.Provider,
    { value: { pending } },
    h("form", { ...rest, ref, method: "post", action: `/_volt/action/${name}`, onSubmit: handleSubmit, "aria-busy": pending || undefined }, children),
  );
});

export default Form;
export { Form };
