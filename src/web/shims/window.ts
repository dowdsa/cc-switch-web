const noop = async () => undefined;

const windowProxy = new Proxy(
  { label: "main", __ccswitchWebShim: true },
  {
    get(target, property) {
      if (property === "label") return "main";
      if (property === "isMaximized" || property === "isFullscreen") {
        return async () => false;
      }
      if (
        property === "onFocusChanged" ||
        property === "onCloseRequested" ||
        property === "onResized" ||
        property === "onMoved"
      ) {
        return async () => () => {};
      }
      if (property in target) {
        return (target as Record<string | symbol, unknown>)[property];
      }
      return noop;
    },
  },
);

export function getCurrentWindow(): typeof windowProxy {
  return windowProxy;
}

export function getAllWindows(): (typeof windowProxy)[] {
  return [windowProxy];
}

export function getCurrentWebviewWindow(): typeof windowProxy {
  return windowProxy;
}
