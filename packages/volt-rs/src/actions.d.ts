export interface Action<Input, Output> {
  (input: Input extends void ? void | FormData : Input | FormData): Promise<Output>;
  readonly $$volt: string;
}

export class ActionError extends Error {
  status: number;
}

export function createAction<Input = void, Output = unknown>(name: string): Action<Input, Output>;

export function useAction<Input, Output>(
  action: Action<Input, Output>,
  options?: { refresh?: boolean },
): {
  run: (input: Input extends void ? void | FormData : Input | FormData) => Promise<Output>;
  pending: boolean;
  data: Output | undefined;
  error: unknown;
};
