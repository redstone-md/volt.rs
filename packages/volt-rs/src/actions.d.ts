type ActionInput<Input> = Input extends void ? void | FormData : Input | FormData;

export interface Action<Input, Output> {
  (input: ActionInput<Input>): Promise<Output>;
  /** Called by React's `useActionState` and `<form action>`: the previous state is ignored. */
  (previousState: unknown, input: ActionInput<Input>): Promise<Output>;
  readonly $$volt: string;
}

/** Per-field validation messages from Rust `Error::validation`. */
export type FieldErrors = Record<string, string[]>;

export class ActionError extends Error {
  status: number;
  /** Set for 422 validation errors. */
  fieldErrors?: FieldErrors;
}

export function createAction<Input = void, Output = unknown>(name: string): Action<Input, Output>;

export function useAction<Input, Output>(
  action: Action<Input, Output>,
  options?: { refresh?: boolean },
): {
  run: (input: ActionInput<Input>) => Promise<Output>;
  pending: boolean;
  data: Output | undefined;
  error: unknown;
  fieldErrors: FieldErrors | undefined;
};

/**
 * React's `useActionState` with volt actions: `[state, formAction, isPending, error]`.
 * An `ActionError` keeps the previous state and is returned as `error`. After a submission without
 * JavaScript the page renders with the returned state (or error); `permalink` posts the form to that page.
 */
export function useActionState<Input, Output, Initial = Output>(
  action: Action<Input, Output>,
  initialState: Initial,
  permalink?: string,
): [state: Output | Initial, formAction: (payload: ActionInput<Input>) => void, isPending: boolean, error: ActionError | undefined];
export function useActionState<State, Payload>(
  action: (state: Awaited<State>, payload: Payload) => State | Promise<State>,
  initialState: Awaited<State>,
  permalink?: string,
): [state: Awaited<State>, formAction: (payload: Payload) => void, isPending: boolean, error: ActionError | undefined];
