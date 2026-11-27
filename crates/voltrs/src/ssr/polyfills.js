(() => {
  const g = globalThis;
  const host = g.__volt_host;

  const format = (args) =>
    args
      .map((a) => {
        if (typeof a === "string") return a;
        if (a instanceof Error) return a.stack || String(a);
        try {
          return JSON.stringify(a);
        } catch {
          return String(a);
        }
      })
      .join(" ");
  g.console = {};
  for (const level of ["log", "info", "warn", "error", "debug", "trace"]) {
    g.console[level] = (...args) => host.log(level, format(args));
  }

  // Timers follow the wall clock: the worker sleeps until the next one is due, so a delayed promise
  // resolves when it would in a browser and its Suspense boundary streams at that moment.
  const timers = new Map();
  let timerId = 0;
  g.setTimeout = (fn, delay = 0, ...args) => {
    const id = ++timerId;
    timers.set(id, { fn, args, due: host.now() + Math.max(0, Number(delay) || 0), seq: id });
    return id;
  };
  g.clearTimeout = (id) => void timers.delete(id);
  g.setInterval = g.setTimeout;
  g.clearInterval = g.clearTimeout;
  g.setImmediate = (fn, ...args) => g.setTimeout(fn, 0, ...args);
  g.clearImmediate = g.clearTimeout;
  g.queueMicrotask ??= (fn) => void Promise.resolve().then(fn);
  /** Runs the earliest due timer and returns 0, else the milliseconds until the next one (-1 when there is none). */
  g.__volt_tick = () => {
    let next = null;
    for (const entry of timers) {
      const [, timer] = entry;
      if (!next || timer.due < next[1].due || (timer.due === next[1].due && timer.seq < next[1].seq)) next = entry;
    }
    if (!next) return -1;
    const wait = next[1].due - host.now();
    if (wait > 0) return Math.ceil(wait);
    timers.delete(next[0]);
    if (typeof next[1].fn === "function") next[1].fn(...next[1].args);
    return 0;
  };

  g.MessageChannel ??= class MessageChannel {
    constructor() {
      const make = () => ({ onmessage: null, postMessage: null, close() {} });
      this.port1 = make();
      this.port2 = make();
      this.port1.postMessage = (data) => g.setTimeout(() => this.port2.onmessage?.({ data }));
      this.port2.postMessage = (data) => g.setTimeout(() => this.port1.onmessage?.({ data }));
    }
  };

  const start = host.now();
  g.performance ??= { now: () => host.now() - start, timeOrigin: start, mark() {}, measure() {} };

  /** UTF-8 bytes of `s` from code unit `start` to `end`; lone surrogates become U+FFFD as in browsers. */
  const encodeUtf8 = (s, start = 0, end = s.length) => {
    const out = new Uint8Array((end - start) * 3);
    let n = 0;
    for (let i = start; i < end; i++) {
      let c = s.charCodeAt(i);
      if (c >= 0xd800 && c <= 0xdbff && i + 1 < end) {
        const low = s.charCodeAt(i + 1);
        if (low >= 0xdc00 && low <= 0xdfff) {
          c = 0x10000 + ((c - 0xd800) << 10) + (low - 0xdc00);
          i++;
        }
      }
      if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
      if (c < 0x80) out[n++] = c;
      else if (c < 0x800) {
        out[n++] = 0xc0 | (c >> 6);
        out[n++] = 0x80 | (c & 63);
      } else if (c < 0x10000) {
        out[n++] = 0xe0 | (c >> 12);
        out[n++] = 0x80 | ((c >> 6) & 63);
        out[n++] = 0x80 | (c & 63);
      } else {
        out[n++] = 0xf0 | (c >> 18);
        out[n++] = 0x80 | ((c >> 12) & 63);
        out[n++] = 0x80 | ((c >> 6) & 63);
        out[n++] = 0x80 | (c & 63);
      }
    }
    return out.slice(0, n);
  };
  g.TextEncoder ??= class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(input = "") {
      const s = String(input);
      return host.encode(s) ?? encodeUtf8(s);
    }
    encodeInto(input, dest) {
      const s = String(input);
      const whole = host.encode(s);
      if (whole && whole.length <= dest.length) {
        dest.set(whole);
        return { read: s.length, written: whole.length };
      }
      let read = 0;
      let written = 0;
      while (read < s.length) {
        const c = s.charCodeAt(read);
        const units = c >= 0xd800 && c <= 0xdbff && read + 1 < s.length ? 2 : 1;
        const bytes = encodeUtf8(s, read, read + units);
        if (written + bytes.length > dest.length) break;
        dest.set(bytes, written);
        written += bytes.length;
        read += units;
      }
      return { read, written };
    }
  };
  /** Length of a UTF-8 sequence cut off at the end of `bytes`, kept back for the next streamed chunk. */
  const incompleteTail = (bytes) => {
    for (let back = 1; back <= Math.min(3, bytes.length); back++) {
      const b = bytes[bytes.length - back];
      if ((b & 0xc0) === 0x80) continue;
      const size = b >= 0xf0 ? 4 : b >= 0xe0 ? 3 : b >= 0xc0 ? 2 : 1;
      return size > back ? back : 0;
    }
    return 0;
  };
  g.TextDecoder ??= class TextDecoder {
    constructor() {
      this.pending = new Uint8Array(0);
    }
    get encoding() {
      return "utf-8";
    }
    decode(input, options) {
      const view = !input
        ? new Uint8Array(0)
        : ArrayBuffer.isView(input)
          ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
          : new Uint8Array(input);
      let bytes = view;
      if (this.pending.length) {
        bytes = new Uint8Array(this.pending.length + view.length);
        bytes.set(this.pending);
        bytes.set(view, this.pending.length);
      }
      const tail = options?.stream ? incompleteTail(bytes) : 0;
      this.pending = bytes.slice(bytes.length - tail);
      return host.decode(bytes.subarray(0, bytes.length - tail));
    }
  };

  const b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  g.btoa ??= (input) => {
    const s = String(input);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const n = (s.charCodeAt(i) << 16) | ((s.charCodeAt(i + 1) || 0) << 8) | (s.charCodeAt(i + 2) || 0);
      out += b64[(n >> 18) & 63] + b64[(n >> 12) & 63];
      out += i + 1 < s.length ? b64[(n >> 6) & 63] : "=";
      out += i + 2 < s.length ? b64[n & 63] : "=";
    }
    return out;
  };
  g.atob ??= (input) => {
    const s = String(input).replace(/[^A-Za-z0-9+/]/g, "");
    let out = "";
    for (let i = 0; i < s.length; i += 4) {
      const n = (b64.indexOf(s[i]) << 18) | (b64.indexOf(s[i + 1]) << 12) | ((b64.indexOf(s[i + 2]) & 63) << 6) | (b64.indexOf(s[i + 3]) & 63);
      out += String.fromCharCode((n >> 16) & 255);
      if (s[i + 2] !== undefined) out += String.fromCharCode((n >> 8) & 255);
      if (s[i + 3] !== undefined) out += String.fromCharCode(n & 255);
    }
    return out;
  };

  g.structuredClone ??= (value) => (value === undefined ? undefined : JSON.parse(JSON.stringify(value)));
  g.crypto ??= {
    getRandomValues(array) {
      for (let i = 0; i < array.length; i++) array[i] = Math.floor(Math.random() * 256);
      return array;
    },
    randomUUID() {
      const h = [...this.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, "0")).join("");
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-4${h.slice(13, 16)}-a${h.slice(17, 20)}-${h.slice(20, 32)}`;
    },
  };

  class URLSearchParams {
    constructor(init = "") {
      this._ = [];
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const [k, v = ""] = part.split("=");
          const dec = (x) => decodeURIComponent(x.replace(/\+/g, " "));
          this._.push([dec(k), dec(v)]);
        }
      } else if (init && typeof init[Symbol.iterator] === "function") {
        for (const [k, v] of init) this._.push([String(k), String(v)]);
      } else if (init) {
        for (const k of Object.keys(init)) this._.push([k, String(init[k])]);
      }
    }
    get size() {
      return this._.length;
    }
    append(k, v) {
      this._.push([String(k), String(v)]);
    }
    delete(k) {
      this._ = this._.filter(([x]) => x !== k);
    }
    get(k) {
      const e = this._.find(([x]) => x === k);
      return e ? e[1] : null;
    }
    getAll(k) {
      return this._.filter(([x]) => x === k).map(([, v]) => v);
    }
    has(k) {
      return this._.some(([x]) => x === k);
    }
    set(k, v) {
      this.delete(k);
      this.append(k, v);
    }
    sort() {
      this._.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
    }
    forEach(fn, self) {
      for (const [k, v] of this._) fn.call(self, v, k, this);
    }
    keys() {
      return this._.map(([k]) => k)[Symbol.iterator]();
    }
    values() {
      return this._.map(([, v]) => v)[Symbol.iterator]();
    }
    entries() {
      return this._.map((e) => [...e])[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    toString() {
      const enc = (x) => encodeURIComponent(x).replace(/%20/g, "+");
      return this._.map(([k, v]) => `${enc(k)}=${enc(v)}`).join("&");
    }
  }
  g.URLSearchParams ??= URLSearchParams;

  class URL {
    constructor(input, base) {
      const re = /^([a-z][a-z0-9+.-]*:)\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i;
      let m = String(input).match(re);
      if (!m && base !== undefined) {
        const b = new URL(base);
        const s = String(input);
        const path = s.startsWith("/") ? s : b.pathname.replace(/[^/]*$/, "") + s;
        m = `${b.origin}${path}`.match(re);
      }
      if (!m) throw new TypeError(`Invalid URL: ${input}`);
      this.protocol = m[1].toLowerCase();
      this.host = m[2];
      this.hostname = m[2].replace(/:\d+$/, "");
      this.port = (m[2].match(/:(\d+)$/) || [])[1] || "";
      this.pathname = m[3] || "/";
      this.search = m[4] && m[4] !== "?" ? m[4] : "";
      this.hash = m[5] || "";
      this.searchParams = new g.URLSearchParams(this.search);
    }
    get origin() {
      return `${this.protocol}//${this.host}`;
    }
    get href() {
      return `${this.origin}${this.pathname}${this.search}${this.hash}`;
    }
    toString() {
      return this.href;
    }
    toJSON() {
      return this.href;
    }
  }
  g.URL ??= URL;

  class EventTarget {
    constructor() {
      this._listeners = [];
    }
    addEventListener(type, listener, options) {
      if (typeof listener !== "function" && typeof listener?.handleEvent !== "function") return;
      const signal = options === Object(options) ? options.signal : undefined;
      if (signal?.aborted) return;
      this._listeners.push({ type, listener, once: options === Object(options) && options.once === true });
      signal?.addEventListener("abort", () => this.removeEventListener(type, listener), { once: true });
    }
    removeEventListener(type, listener) {
      this._listeners = this._listeners.filter((entry) => entry.type !== type || entry.listener !== listener);
    }
    dispatchEvent(event) {
      for (const entry of [...this._listeners]) {
        if (entry.type !== event.type) continue;
        if (entry.once) this.removeEventListener(entry.type, entry.listener);
        if (typeof entry.listener === "function") entry.listener.call(this, event);
        else entry.listener.handleEvent(event);
      }
      const handler = this[`on${event.type}`];
      if (typeof handler === "function") handler.call(this, event);
      return true;
    }
  }
  g.EventTarget ??= EventTarget;

  class AbortSignal extends EventTarget {
    constructor() {
      super();
      this.aborted = false;
      this.reason = undefined;
      this.onabort = null;
    }
    throwIfAborted() {
      if (this.aborted) throw this.reason;
    }
    static abort(reason) {
      const controller = new g.AbortController();
      controller.abort(reason);
      return controller.signal;
    }
  }
  g.AbortSignal ??= AbortSignal;
  g.AbortController ??= class AbortController {
    constructor() {
      this.signal = new AbortSignal();
    }
    abort(reason) {
      const signal = this.signal;
      if (signal.aborted) return;
      signal.aborted = true;
      signal.reason = reason ?? Object.assign(new Error("This operation was aborted"), { name: "AbortError" });
      signal.dispatchEvent({ type: "abort", target: signal });
    }
  };

  /** The part of WHATWG readable streams that `renderToReadableStream` and its readers use. */
  class ReadableStream {
    constructor(source = {}, strategy = {}) {
      this._source = source;
      this._queue = [];
      this._reads = [];
      this._state = "readable";
      this._closing = false;
      this._started = false;
      this._pulling = false;
      this._pullAgain = false;
      this._highWaterMark = strategy.highWaterMark ?? (source.type === "bytes" ? 0 : 1);
      this._reader = null;
      const stream = this;
      this._controller = {
        byobRequest: null,
        get desiredSize() {
          if (stream._state === "errored") return null;
          return stream._state === "closed" ? 0 : stream._highWaterMark - stream._queue.length;
        },
        enqueue(chunk) {
          if (stream._state !== "readable" || stream._closing) throw new TypeError("the stream is not readable");
          const read = stream._reads.shift();
          if (read) read.resolve({ value: chunk, done: false });
          else stream._queue.push(chunk);
          stream._pullIfNeeded();
        },
        close() {
          if (stream._state !== "readable" || stream._closing) return;
          stream._closing = true;
          if (stream._queue.length === 0) stream._close();
        },
        error(error) {
          stream._fail(error);
        },
      };
      Promise.resolve()
        .then(() => (typeof source.start === "function" ? source.start(this._controller) : undefined))
        .then(
          () => {
            this._started = true;
            this._pullIfNeeded();
          },
          (error) => this._fail(error),
        );
    }
    get locked() {
      return this._reader !== null;
    }
    _close() {
      this._state = "closed";
      for (const read of this._reads.splice(0)) read.resolve({ value: undefined, done: true });
      this._reader?._settle(null);
    }
    _fail(error) {
      if (this._state !== "readable") return;
      this._state = "errored";
      this._error = error;
      this._queue = [];
      for (const read of this._reads.splice(0)) read.reject(error);
      this._reader?._settle(error);
    }
    _pullIfNeeded() {
      if (!this._started || this._state !== "readable" || this._closing || typeof this._source.pull !== "function") return;
      if (this._reads.length === 0 && this._queue.length >= this._highWaterMark) return;
      if (this._pulling) {
        this._pullAgain = true;
        return;
      }
      this._pulling = true;
      Promise.resolve()
        .then(() => this._source.pull(this._controller))
        .then(
          () => {
            this._pulling = false;
            if (this._pullAgain) {
              this._pullAgain = false;
              this._pullIfNeeded();
            }
          },
          (error) => this._fail(error),
        );
    }
    _read() {
      if (this._queue.length > 0) {
        const value = this._queue.shift();
        if (this._closing && this._queue.length === 0) this._close();
        else this._pullIfNeeded();
        return Promise.resolve({ value, done: false });
      }
      if (this._state === "closed") return Promise.resolve({ value: undefined, done: true });
      if (this._state === "errored") return Promise.reject(this._error);
      return new Promise((resolve, reject) => {
        this._reads.push({ resolve, reject });
        this._pullIfNeeded();
      });
    }
    cancel(reason) {
      if (this._state === "closed") return Promise.resolve();
      if (this._state === "errored") return Promise.reject(this._error);
      this._queue = [];
      this._close();
      const cancel = typeof this._source.cancel === "function" ? this._source.cancel(reason) : undefined;
      return Promise.resolve(cancel).then(() => undefined);
    }
    getReader() {
      if (this._reader) throw new TypeError("the stream is locked to a reader");
      const stream = this;
      let settle;
      const closed = new Promise((resolve, reject) => {
        settle = (error) => (error === null ? resolve() : reject(error));
      });
      closed.catch(() => {});
      const reader = {
        closed,
        _settle: settle,
        read: () => stream._read(),
        cancel: (reason) => stream.cancel(reason),
        releaseLock() {
          if (stream._reader === reader) stream._reader = null;
        },
      };
      this._reader = reader;
      if (this._state === "closed") settle(null);
      else if (this._state === "errored") settle(this._error);
      return reader;
    }
    async *[Symbol.asyncIterator]() {
      const reader = this.getReader();
      try {
        for (;;) {
          const { value, done } = await reader.read();
          if (done) return;
          yield value;
        }
      } finally {
        reader.releaseLock();
      }
    }
  }
  g.ReadableStream ??= ReadableStream;
  g.process ??= { env: {} };
  g.process.env.NODE_ENV ??= host.env;
})();
