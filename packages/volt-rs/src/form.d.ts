import type { FormHTMLAttributes, ForwardRefExoticComponent, RefAttributes } from "react";
import type { Action } from "./actions";

export interface FormProps<Output = unknown> extends Omit<FormHTMLAttributes<HTMLFormElement>, "action"> {
  action: Action<any, Output> | string;
  /** Re-run the route loader after a successful submit. Defaults to `true`. */
  refresh?: boolean;
  onSuccess?: (result: Output, form: HTMLFormElement) => void;
  onError?: (error: unknown) => void;
}

declare const Form: ForwardRefExoticComponent<FormProps & RefAttributes<HTMLFormElement>>;
export function useFormPending(): boolean;
export default Form;
export { Form };
