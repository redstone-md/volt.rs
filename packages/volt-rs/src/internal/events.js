/** Minimal event emitter, compatible with the `mitt` instance behind `next/router` events. */
export function createEvents() {
  const handlers = new Map();
  return {
    on(type, handler) {
      if (!handlers.has(type)) handlers.set(type, new Set());
      handlers.get(type).add(handler);
    },
    off(type, handler) {
      handlers.get(type)?.delete(handler);
    },
    emit(type, ...args) {
      for (const handler of [...(handlers.get(type) ?? [])]) handler(...args);
    },
  };
}

/** Navigation lifecycle events shared by every router API. */
export const routerEvents = createEvents();
