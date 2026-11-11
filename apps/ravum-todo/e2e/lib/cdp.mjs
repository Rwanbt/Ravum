// Minimal Chrome DevTools Protocol client built on Node's built-in `WebSocket`.
//
// Zero npm dependency: Node >= 22 exposes `WebSocket` and `fetch` globally, which is
// all this harness needs. The client supports the two things the harness uses:
// request/response commands (with timeouts) and event subscriptions, including the
// flattened session ids returned by `Target.attachToTarget { flatten: true }`.

const DEFAULT_COMMAND_TIMEOUT_MS = 30_000;

export class CdpError extends Error {
  constructor(method, error) {
    super(
      `CDP ${method} a échoué: ${error?.message ?? JSON.stringify(error)}` +
        (error?.code === undefined ? '' : ` (code ${error.code})`),
    );
    this.name = 'CdpError';
    this.method = method;
    this.code = error?.code ?? null;
    this.data = error?.data ?? null;
  }
}

export class CdpConnection {
  #socket;
  #nextId = 1;
  #pending = new Map();
  #listeners = new Map();
  #closed = false;
  #closeError = null;

  /** Connect to a browser-level (or page-level) DevTools WebSocket URL. */
  static async connect(url, { timeoutMs = 20_000 } = {}) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      let settled = false;
      const finish = (callback, value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        socket.removeEventListener('open', onOpen);
        socket.removeEventListener('error', onError);
        socket.removeEventListener('close', onClose);
        callback(value);
      };
      const timer = setTimeout(
        () => finish(reject, new Error(`CDP: délai de connexion dépassé (${url})`)),
        timeoutMs,
      );
      const onOpen = () => finish(resolve);
      const onError = () => finish(reject, new Error(`CDP: connexion WebSocket refusée (${url})`));
      const onClose = (event) =>
        finish(
          reject,
          new Error(`CDP: connexion fermée pendant le handshake (code ${event?.code ?? '?'})`),
        );
      socket.addEventListener('open', onOpen);
      socket.addEventListener('error', onError);
      socket.addEventListener('close', onClose);
    });
    return new CdpConnection(socket);
  }

  constructor(socket) {
    this.#socket = socket;
    this.#socket.addEventListener('message', (event) => this.#handleMessage(event.data));
    this.#socket.addEventListener('close', (event) => {
      this.#closed = true;
      this.#closeError = new Error(`CDP: connexion fermée (code ${event?.code ?? '?'})`);
      for (const [id, pending] of this.#pending) {
        this.#pending.delete(id);
        clearTimeout(pending.timer);
        pending.reject(this.#closeError);
      }
    });
    this.#socket.addEventListener('error', () => {
      // A close event follows; pending commands are rejected there. Keep the listener
      // so Node does not treat the error as unhandled.
    });
  }

  get closed() {
    return this.#closed;
  }

  /**
   * Send one command. `options.sessionId` routes the command to a flattened page
   * session; `options.timeoutMs` bounds the wait for the matching response.
   */
  async send(method, params = {}, { sessionId, timeoutMs = DEFAULT_COMMAND_TIMEOUT_MS } = {}) {
    if (this.#closed) {
      throw this.#closeError ?? new Error('CDP: connexion déjà fermée');
    }
    const id = this.#nextId++;
    const message = { id, method, params };
    if (sessionId) message.sessionId = sessionId;
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`CDP ${method}: délai dépassé (${timeoutMs} ms)`));
      }, timeoutMs);
      this.#pending.set(id, { resolve, reject, method, timer });
      try {
        this.#socket.send(JSON.stringify(message));
      } catch (error) {
        clearTimeout(timer);
        this.#pending.delete(id);
        reject(error);
      }
    });
  }

  /**
   * Subscribe to one CDP event method (`'*'` for every event). The handler receives
   * `(params, message)`; the returned function unsubscribes.
   */
  on(method, handler) {
    let handlers = this.#listeners.get(method);
    if (!handlers) {
      handlers = new Set();
      this.#listeners.set(method, handlers);
    }
    handlers.add(handler);
    return () => handlers.delete(handler);
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    try {
      this.#socket.close();
    } catch {
      // Best effort: the process is about to be killed anyway.
    }
    this.#closeError = new Error('CDP: connexion fermée par le harnais');
    for (const [id, pending] of this.#pending) {
      this.#pending.delete(id);
      clearTimeout(pending.timer);
      pending.reject(this.#closeError);
    }
  }

  #handleMessage(raw) {
    let message;
    try {
      message = JSON.parse(typeof raw === 'string' ? raw : String(raw));
    } catch {
      return;
    }
    if (typeof message.id === 'number') {
      const pending = this.#pending.get(message.id);
      if (!pending) return;
      this.#pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error) pending.reject(new CdpError(pending.method, message.error));
      else pending.resolve(message.result ?? {});
      return;
    }
    const handlers = this.#listeners.get(message.method);
    if (handlers) {
      for (const handler of [...handlers]) {
        try {
          handler(message.params ?? {}, message);
        } catch {
          // A failing observer must never break the protocol loop.
        }
      }
    }
    const wildcard = this.#listeners.get('*');
    if (wildcard) {
      for (const handler of [...wildcard]) {
        try {
          handler(message.params ?? {}, message);
        } catch {
          // Same as above.
        }
      }
    }
  }
}
