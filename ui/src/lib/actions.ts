// Action handler registry (BUILD_PLAN §2.4). Lanes register handlers from their `actions.ts`
// modules (loaded eagerly by main.ts); the key manager, palette and toasts call `dispatch`.

import {
  ACTIONS,
  PLUGIN_COMMAND_PREFIX,
  type ActionId,
  type ActionMeta,
  type BuiltinActionId,
} from '$lib/gen/actions';

export type ActionArgs = Record<string, unknown> | undefined;
export type ActionHandler = (args?: ActionArgs) => void | Promise<void>;

const handlers = new Map<string, ActionHandler>();

/** Registers the handler of an action id. Returns an unregister function. Last registration wins. */
export function registerAction(id: ActionId | string, handler: ActionHandler): () => void {
  if (handlers.has(id)) console.warn(`[kelta] action ${id} registered twice; replacing`);
  handlers.set(id, handler);
  return () => {
    if (handlers.get(id) === handler) handlers.delete(id);
  };
}

export function hasAction(id: string): boolean {
  return (
    handlers.has(id) || (id.startsWith(PLUGIN_COMMAND_PREFIX) && handlers.has(`${PLUGIN_COMMAND_PREFIX}*`))
  );
}

/**
 * Runs an action. Returns false when no handler is registered. `plugin.command.<id>` falls back to
 * the `plugin.command.*` handler (registered by L8) with `{ command_id }` merged into the args.
 */
export async function dispatch(id: ActionId | string, args?: ActionArgs): Promise<boolean> {
  const direct = handlers.get(id);
  if (direct) {
    await direct(args);
    return true;
  }
  if (id.startsWith(PLUGIN_COMMAND_PREFIX)) {
    const generic = handlers.get(`${PLUGIN_COMMAND_PREFIX}*`);
    if (generic) {
      await generic({ ...(args ?? {}), command_id: id.slice(PLUGIN_COMMAND_PREFIX.length) });
      return true;
    }
  }
  return false;
}

/** Registered action ids (palette listing). */
export function registeredActions(): string[] {
  return [...handlers.keys()];
}

export function actionMeta(id: string): ActionMeta | null {
  return ACTIONS.find((a) => a.id === id) ?? null;
}

export function isBuiltinAction(id: string): id is BuiltinActionId {
  return ACTIONS.some((a) => a.id === id);
}

/** Test helper. */
export function clearActionsForTests(): void {
  handlers.clear();
}
