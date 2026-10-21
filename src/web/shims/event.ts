type EventPayload<T> = {
  event: string;
  id: number;
  payload: T;
};

type Handler = (event: EventPayload<unknown>) => void;

export type UnlistenFn = () => void;

const handlers = new Map<string, Set<Handler>>();
let source: EventSource | null = null;
let eventId = 0;

function ensureSource(): void {
  if (source) return;
  source = new EventSource("/api/events");
  source.onmessage = (message) => {
    try {
      const data = JSON.parse(message.data) as {
        event?: string;
        payload?: unknown;
      };
      if (!data?.event) return;
      if (data.event === "resync") {
        window.dispatchEvent(new Event("cc:resync"));
        return;
      }
      const set = handlers.get(data.event);
      if (!set) return;
      const event: EventPayload<unknown> = {
        event: data.event,
        id: ++eventId,
        payload: data.payload,
      };
      set.forEach((handler) => {
        try {
          handler(event);
        } catch (error) {
          console.error("[cc-switch-web] event handler failed", error);
        }
      });
    } catch {
      /* 忽略无法解析的事件 */
    }
  };
}

export async function listen<T>(
  event: string,
  handler: (event: EventPayload<T>) => void,
): Promise<UnlistenFn> {
  ensureSource();
  let set = handlers.get(event);
  if (!set) {
    set = new Set();
    handlers.set(event, set);
  }
  const wrapped = handler as Handler;
  set.add(wrapped);
  return () => {
    set?.delete(wrapped);
  };
}

export async function once<T>(
  event: string,
  handler: (event: EventPayload<T>) => void,
): Promise<UnlistenFn> {
  let unlisten: UnlistenFn | null = null;
  const stop = await listen<T>(event, (payload) => {
    handler(payload);
    unlisten?.();
  });
  unlisten = stop;
  return stop;
}
