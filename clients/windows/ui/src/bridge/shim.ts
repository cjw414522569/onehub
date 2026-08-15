// PC GUI bridge shim (adapter added on top of the mXterm copy).
// Installs window.__TAURI_INTERNALS__ so @tauri-apps/api calls route to the
// native host bridge (window.chrome.webview.postMessage) instead of a real
// Tauri runtime. With no bridge present, commands resolve to benign defaults
// so the UI still renders. The host injects this script before the app boots.

interface WebViewMessageEvent extends MessageEvent {
  data: { kind?: string; requestId?: number; handlerId?: number; payload?: unknown; error?: string };
}

interface SshBridgeWindow extends Window {
  chrome?: {
    webview?: {
      postMessage(message: unknown): void;
      addEventListener?(type: "message", listener: (event: WebViewMessageEvent) => void): void;
    };
  };
  __TAURI_INTERNALS__?: {
    metadata?: {
      currentWindow: { label: string };
      currentWebview: { label: string };
    };
    invoke(cmd: string, payload?: unknown, options?: unknown): Promise<unknown>;
    transformCallback(callback?: (response: unknown) => void, once?: boolean): number;
    unregisterCallback(id: number): void;
    runCallback?(id: number, payload?: unknown): void;
    convertFileSrc(filePath: string, protocol?: string): string;
    postMessage(message: unknown): void;
  };
}

const globalWindow = window as SshBridgeWindow;

let callbackCounter = 0;
const callbacks = new Map<number, (response: unknown) => void>();
const pendingInvokes = new Map<number, (value: unknown) => void>();
const eventCallbacks = new Map<number, (payload: unknown) => void>();

function bridgeAvailable(): boolean {
  return Boolean(globalWindow.chrome?.webview?.postMessage);
}

function postToHost(message: unknown): void {
  globalWindow.chrome?.webview?.postMessage(message);
}

function install(): void {
  if (globalWindow.__TAURI_INTERNALS__) return;

  globalWindow.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
    transformCallback(callback, _once) {
      const id = ++callbackCounter;
      if (callback) callbacks.set(id, callback);
      if (callback) eventCallbacks.set(id, callback as (payload: unknown) => void);
      return id;
    },
    runCallback(id, payload) {
      eventCallbacks.get(id)?.(payload);
    },
    unregisterCallback(_id: number) {},
    convertFileSrc(filePath: string) {
      return filePath;
    },
    postMessage(message) {
      postToHost(message);
    },
    invoke(cmd, payload, _options) {
      if (bridgeAvailable()) {
        return new Promise<unknown>((resolve) => {
          const internals = globalWindow.__TAURI_INTERNALS__ as NonNullable<
            SshBridgeWindow["__TAURI_INTERNALS__"]
          > & { _nextRequestId?: number };
          internals._nextRequestId = (internals._nextRequestId ?? 0) + 1;
          const requestId = internals._nextRequestId;
          pendingInvokes.set(requestId, resolve);
          postToHost({ kind: "invoke", requestId, cmd, payload });
        });
      }
      // No host bridge: benign default so the UI renders.
      return Promise.resolve(null);
    },
  };
}

install();

// Event plugin internals required by @tauri-apps/api/event unlisten.
(globalWindow as SshBridgeWindow & {
  __TAURI_EVENT_PLUGIN_INTERNALS__?: {
    unregisterListener(event: string, eventId: number): Promise<unknown>;
  };
}).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener(event, eventId) {
    return globalWindow.__TAURI_INTERNALS__!.invoke("plugin:event|unlisten", { event, eventId });
  },
};

// Route host replies back to pending invoke promises.
globalWindow.chrome?.webview?.addEventListener?.("message", (event: WebViewMessageEvent) => {
  const data = event.data;
  if (data?.kind === "event" && typeof data.handlerId === "number") {
    globalWindow.__TAURI_INTERNALS__?.runCallback?.(data.handlerId, data.payload);
    return;
  }
  if (data?.kind === "invoke-reply" && typeof data.requestId === "number") {
    const resolve = pendingInvokes.get(data.requestId);
    if (resolve) {
      pendingInvokes.delete(data.requestId);
      resolve(data.error ? new Error(data.error) : data.payload);
    }
  }
});

export {};