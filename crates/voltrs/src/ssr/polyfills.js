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

  const formEncode = (x) => encodeURIComponent(x).replace(/%20/g, "+").replace(/[!'()~]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
  const formDecode = (x) => {
    const s = x.replace(/\+/g, " ");
    try {
      return decodeURIComponent(s);
    } catch {
      return s;
    }
  };

  class URLSearchParams {
    constructor(init = "") {
      this._ = [];
      this._url = null;
      if (typeof init === "string") this._parse(init);
      else if (init instanceof URLSearchParams) this._ = init._.map((e) => [...e]);
      else if (init && typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const [k, v] = [...pair];
          this._.push([String(k), String(v)]);
        }
      } else if (init) {
        for (const k of Object.keys(init)) this._.push([k, String(init[k])]);
      }
    }
    _parse(text) {
      this._ = [];
      for (const part of String(text).replace(/^\?/, "").split("&")) {
        if (!part) continue;
        const index = part.indexOf("=");
        const [k, v] = index < 0 ? [part, ""] : [part.slice(0, index), part.slice(index + 1)];
        this._.push([formDecode(k), formDecode(v)]);
      }
    }
    /** Writes changes back to the URL this object belongs to. */
    _changed() {
      if (this._url) this._url._search = this._.length ? `?${this}` : "";
    }
    get size() {
      return this._.length;
    }
    append(k, v) {
      this._.push([String(k), String(v)]);
      this._changed();
    }
    delete(k, v) {
      this._ = this._.filter(([x, y]) => x !== String(k) || (v !== undefined && y !== String(v)));
      this._changed();
    }
    get(k) {
      const e = this._.find(([x]) => x === String(k));
      return e ? e[1] : null;
    }
    getAll(k) {
      return this._.filter(([x]) => x === String(k)).map(([, v]) => v);
    }
    has(k, v) {
      return this._.some(([x, y]) => x === String(k) && (v === undefined || y === String(v)));
    }
    set(k, v) {
      const key = String(k);
      const index = this._.findIndex(([x]) => x === key);
      if (index < 0) this._.push([key, String(v)]);
      else {
        this._[index] = [key, String(v)];
        this._ = this._.filter(([x], i) => x !== key || i <= index);
      }
      this._changed();
    }
    sort() {
      this._.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
      this._changed();
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
      return this._.map(([k, v]) => `${formEncode(k)}=${formEncode(v)}`).join("&");
    }
  }
  g.URLSearchParams ??= URLSearchParams;

  const DEFAULT_PORTS = { "http:": "80", "https:": "443", "ws:": "80", "wss:": "443" };

  /** Resolves `.` and `..` segments of an absolute path. */
  const normalizePath = (path) => {
    const out = [];
    const parts = path.split("/");
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const last = i === parts.length - 1;
      if (part === "..") {
        if (out.length > 1) out.pop();
        if (last) out.push("");
      } else if (part === ".") {
        if (last) out.push("");
      } else out.push(part);
    }
    const joined = out.join("/");
    return joined.startsWith("/") ? joined : `/${joined}`;
  };
  const encodePath = (path) => path.replace(/[^A-Za-z0-9\-._~!$&'()*+,;=:@/%]/g, (c) => encodeURIComponent(c));

  /** WHATWG URL for special schemes (`http`, `https`, ...) and opaque ones such as `data:` or `mailto:`. */
  class URL {
    constructor(input, base) {
      const text = String(input).trim();
      const absolute = /^[a-z][a-z0-9+.-]*:/i.test(text);
      if (!absolute) {
        if (base === undefined) throw new TypeError(`Invalid URL: ${input}`);
        const b = base instanceof URL ? base : new URL(base);
        if (!b._special) throw new TypeError(`Invalid URL: ${input}`);
        let resolved;
        if (text.startsWith("//")) resolved = `${b.protocol}${text}`;
        else if (text.startsWith("/")) resolved = `${b.origin}${text}`;
        else if (text.startsWith("?")) resolved = `${b.origin}${b.pathname}${text}`;
        else if (text.startsWith("#")) resolved = `${b.origin}${b.pathname}${b.search}${text}`;
        else if (text === "") resolved = `${b.origin}${b.pathname}${b.search}`;
        else resolved = `${b.origin}${b.pathname.replace(/[^/]*$/, "")}${text}`;
        this._set(resolved);
        return;
      }
      this._set(text);
    }
    _set(text) {
      const special = text.match(/^([a-z][a-z0-9+.-]*:)\/\/(?:([^@/?#]*)@)?([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i);
      if (special) {
        this._special = true;
        this._protocol = special[1].toLowerCase();
        const [user = "", pass = ""] = (special[2] ?? "").split(":");
        this.username = user;
        this.password = pass;
        this.host = special[3];
        this._pathname = normalizePath(encodePath(special[4] || "/"));
        this.search = special[5] ?? "";
        this.hash = special[6] ?? "";
        return;
      }
      const opaque = text.match(/^([a-z][a-z0-9+.-]*:)([^?#]*)(\?[^#]*)?(#.*)?$/i);
      if (!opaque) throw new TypeError(`Invalid URL: ${text}`);
      this._special = false;
      this._protocol = opaque[1].toLowerCase();
      this.username = "";
      this.password = "";
      this._hostname = "";
      this._port = "";
      this._pathname = opaque[2];
      this.search = opaque[3] ?? "";
      this.hash = opaque[4] ?? "";
    }
    get protocol() {
      return this._protocol;
    }
    set protocol(value) {
      const protocol = String(value).replace(/:?$/, ":").toLowerCase();
      if (/^[a-z][a-z0-9+.-]*:$/.test(protocol)) this._protocol = protocol;
    }
    get hostname() {
      return this._hostname;
    }
    set hostname(value) {
      this._hostname = String(value).toLowerCase();
    }
    get port() {
      return this._port;
    }
    set port(value) {
      const port = String(value).replace(/\D.*$/, "");
      this._port = port === DEFAULT_PORTS[this._protocol] ? "" : port;
    }
    get host() {
      return this._port ? `${this._hostname}:${this._port}` : this._hostname;
    }
    set host(value) {
      const match = String(value).match(/^(\[[^\]]*\]|[^:]*)(?::(\d*))?$/);
      if (!match) return;
      this.hostname = match[1];
      this.port = match[2] ?? "";
    }
    get pathname() {
      return this._pathname;
    }
    set pathname(value) {
      const path = String(value);
      this._pathname = this._special ? normalizePath(encodePath(path.startsWith("/") ? path : `/${path}`)) : path;
    }
    get search() {
      return this._search;
    }
    set search(value) {
      const search = String(value);
      this._search = search === "" || search === "?" ? "" : search.startsWith("?") ? search : `?${search}`;
      if (!this._params) {
        this._params = new g.URLSearchParams(this._search);
        this._params._url = this;
      } else this._params._parse(this._search);
    }
    get searchParams() {
      return this._params;
    }
    get hash() {
      return this._hash;
    }
    set hash(value) {
      const hash = String(value);
      this._hash = hash === "" || hash === "#" ? "" : hash.startsWith("#") ? hash : `#${hash}`;
    }
    get origin() {
      return this._special ? `${this._protocol}//${this.host}` : "null";
    }
    get href() {
      if (!this._special) return `${this._protocol}${this._pathname}${this._search}${this._hash}`;
      const credentials = this.username ? `${this.username}${this.password ? `:${this.password}` : ""}@` : "";
      return `${this._protocol}//${credentials}${this.host}${this._pathname}${this._search}${this._hash}`;
    }
    set href(value) {
      this._set(String(value));
    }
    toString() {
      return this.href;
    }
    toJSON() {
      return this.href;
    }
    static canParse(input, base) {
      try {
        new URL(input, base);
        return true;
      } catch {
        return false;
      }
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

  /** Bytes of a buffer source as a `Uint8Array` view. */
  const toBytes = (value) =>
    value instanceof ArrayBuffer
      ? new Uint8Array(value)
      : new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  const concatBytes = (chunks) => {
    const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
    let offset = 0;
    for (const chunk of chunks) {
      out.set(chunk, offset);
      offset += chunk.length;
    }
    return out;
  };
  const utf8 = (text) => new g.TextEncoder().encode(text);
  const fromUtf8 = (bytes) => new g.TextDecoder().decode(bytes);

  class Blob {
    constructor(parts = [], options = {}) {
      const chunks = [];
      for (const part of parts) {
        if (part instanceof Blob) chunks.push(part._bytes);
        else if (part instanceof ArrayBuffer || ArrayBuffer.isView(part)) chunks.push(toBytes(part).slice());
        else chunks.push(utf8(String(part)));
      }
      this._bytes = concatBytes(chunks);
      this.type = options.type ? String(options.type).toLowerCase() : "";
    }
    get size() {
      return this._bytes.length;
    }
    slice(start = 0, end = this.size, type = "") {
      const blob = new Blob([], { type });
      blob._bytes = this._bytes.slice(start, end);
      return blob;
    }
    async text() {
      return fromUtf8(this._bytes);
    }
    async arrayBuffer() {
      return this._bytes.slice().buffer;
    }
    async bytes() {
      return this._bytes.slice();
    }
    stream() {
      const bytes = this._bytes.slice();
      return new g.ReadableStream({
        start(controller) {
          if (bytes.length) controller.enqueue(bytes);
          controller.close();
        },
      });
    }
  }
  g.Blob ??= Blob;
  g.File ??= class File extends g.Blob {
    constructor(parts, name, options = {}) {
      super(parts, options);
      this.name = String(name);
      this.lastModified = options.lastModified ?? Date.now();
    }
  };

  class FormData {
    constructor() {
      this._ = [];
    }
    _value(value, filename) {
      if (!(value instanceof g.Blob)) return String(value);
      if (filename === undefined && value instanceof g.File) return value;
      return new g.File([value], filename ?? "blob", { type: value.type });
    }
    append(name, value, filename) {
      this._.push([String(name), this._value(value, filename)]);
    }
    set(name, value, filename) {
      this.delete(name);
      this.append(name, value, filename);
    }
    delete(name) {
      this._ = this._.filter(([k]) => k !== String(name));
    }
    get(name) {
      const entry = this._.find(([k]) => k === String(name));
      return entry ? entry[1] : null;
    }
    getAll(name) {
      return this._.filter(([k]) => k === String(name)).map(([, v]) => v);
    }
    has(name) {
      return this._.some(([k]) => k === String(name));
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
  }
  g.FormData ??= FormData;

  /** `multipart/form-data` body of a form, with its boundary. */
  const multipart = (form) => {
    const boundary = `----volt${Math.random().toString(16).slice(2)}${Math.random().toString(16).slice(2)}`;
    const quote = (s) => s.replace(/"/g, "%22").replace(/\r?\n/g, "%0A");
    const chunks = [];
    for (const [name, value] of form) {
      let head = `--${boundary}\r\nContent-Disposition: form-data; name="${quote(name)}"`;
      if (typeof value === "string") {
        chunks.push(utf8(`${head}\r\n\r\n${value}\r\n`));
        continue;
      }
      head += `; filename="${quote(value.name)}"\r\nContent-Type: ${value.type || "application/octet-stream"}\r\n\r\n`;
      chunks.push(utf8(head), value._bytes, utf8("\r\n"));
    }
    chunks.push(utf8(`--${boundary}--\r\n`));
    return { bytes: concatBytes(chunks), type: `multipart/form-data; boundary=${boundary}` };
  };

  const indexOfBytes = (haystack, needle, from) => {
    outer: for (let i = from; i <= haystack.length - needle.length; i++) {
      for (let j = 0; j < needle.length; j++) if (haystack[i + j] !== needle[j]) continue outer;
      return i;
    }
    return -1;
  };

  /** Parses a `multipart/form-data` body. */
  const parseMultipart = (bytes, boundary) => {
    const form = new g.FormData();
    const delimiter = utf8(`--${boundary}`);
    let start = indexOfBytes(bytes, delimiter, 0);
    while (start >= 0) {
      const partStart = start + delimiter.length + 2;
      const end = indexOfBytes(bytes, delimiter, partStart);
      if (end < 0 || bytes[start + delimiter.length] === 45) break;
      const part = bytes.subarray(partStart, end - 2);
      const split = indexOfBytes(part, utf8("\r\n\r\n"), 0);
      const head = fromUtf8(part.subarray(0, split < 0 ? part.length : split));
      const content = split < 0 ? new Uint8Array(0) : part.subarray(split + 4);
      const name = head.match(/name="([^"]*)"/i)?.[1];
      const filename = head.match(/filename="([^"]*)"/i)?.[1];
      const type = head.match(/content-type:\s*([^\r\n]+)/i)?.[1] ?? "";
      if (name !== undefined) {
        form.append(name, filename === undefined ? fromUtf8(content) : new g.File([content], filename, { type }));
      }
      start = end;
    }
    return form;
  };

  const normalizeName = (name) => {
    const text = String(name).toLowerCase();
    if (!/^[!#$%&'*+\-.^_`|~0-9a-z]+$/.test(text)) throw new TypeError(`Invalid header name: ${name}`);
    return text;
  };
  const normalizeValue = (value) => String(value).replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, "");

  class Headers {
    constructor(init) {
      this._ = [];
      if (init instanceof Headers) this._ = init._.map((e) => [...e]);
      else if (init && typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const [name, value] = [...pair];
          this.append(name, value);
        }
      } else if (init) for (const name of Object.keys(init)) this.append(name, init[name]);
    }
    append(name, value) {
      this._.push([normalizeName(name), normalizeValue(value)]);
    }
    set(name, value) {
      const key = normalizeName(name);
      const index = this._.findIndex(([k]) => k === key);
      if (index < 0) return this.append(name, value);
      this._[index] = [key, normalizeValue(value)];
      this._ = this._.filter(([k], i) => k !== key || i <= index);
    }
    delete(name) {
      const key = normalizeName(name);
      this._ = this._.filter(([k]) => k !== key);
    }
    get(name) {
      const key = normalizeName(name);
      const values = this._.filter(([k]) => k === key).map(([, v]) => v);
      return values.length ? values.join(", ") : null;
    }
    getSetCookie() {
      return this._.filter(([k]) => k === "set-cookie").map(([, v]) => v);
    }
    has(name) {
      const key = normalizeName(name);
      return this._.some(([k]) => k === key);
    }
    /** Sorted, combined entries as the Fetch standard iterates them; `set-cookie` values stay separate. */
    entries() {
      const names = [...new Set(this._.map(([k]) => k))].sort();
      const out = [];
      for (const name of names) {
        if (name === "set-cookie") for (const value of this.getSetCookie()) out.push([name, value]);
        else out.push([name, this.get(name)]);
      }
      return out[Symbol.iterator]();
    }
    keys() {
      return [...this.entries()].map(([k]) => k)[Symbol.iterator]();
    }
    values() {
      return [...this.entries()].map(([, v]) => v)[Symbol.iterator]();
    }
    forEach(fn, self) {
      for (const [k, v] of this.entries()) fn.call(self, v, k, this);
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }
  g.Headers ??= Headers;

  /** Turns a body init into bytes or a stream, with the content type it implies. */
  const extractBody = (body) => {
    if (body == null) return { bytes: null, stream: null, type: null };
    if (typeof body === "string") return { bytes: utf8(body), type: "text/plain;charset=UTF-8" };
    if (body instanceof g.URLSearchParams) {
      return { bytes: utf8(body.toString()), type: "application/x-www-form-urlencoded;charset=UTF-8" };
    }
    if (body instanceof g.FormData) return multipart(body);
    if (body instanceof g.Blob) return { bytes: body._bytes.slice(), type: body.type || null };
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return { bytes: toBytes(body).slice(), type: null };
    if (body instanceof g.ReadableStream) return { stream: body, type: null };
    return { bytes: utf8(String(body)), type: "text/plain;charset=UTF-8" };
  };

  const readStream = async (stream) => {
    const chunks = [];
    for await (const chunk of stream) {
      chunks.push(typeof chunk === "string" ? utf8(chunk) : toBytes(chunk));
    }
    return concatBytes(chunks);
  };

  /** The body of a `Request` or `Response`: bytes, or a stream read once on demand. */
  class Body {
    _init(body, headers) {
      const extracted = extractBody(body);
      this._bytes = extracted.bytes ?? null;
      this._stream = extracted.stream ?? null;
      this._used = false;
      if (extracted.type && !headers.has("content-type")) headers.set("content-type", extracted.type);
    }
    get bodyUsed() {
      return this._used;
    }
    get body() {
      if (this._stream) return this._stream;
      if (this._bytes === null) return null;
      const bytes = this._bytes;
      this._stream = new g.ReadableStream({
        start(controller) {
          if (bytes.length) controller.enqueue(bytes);
          controller.close();
        },
      });
      this._bytes = null;
      return this._stream;
    }
    async _consume() {
      if (this._used) throw new TypeError("Body has already been consumed");
      this._used = true;
      if (this._stream) return readStream(this._stream);
      return this._bytes ?? new Uint8Array(0);
    }
    async bytes() {
      return (await this._consume()).slice();
    }
    async arrayBuffer() {
      return (await this._consume()).slice().buffer;
    }
    async text() {
      return fromUtf8(await this._consume());
    }
    async json() {
      return JSON.parse(await this.text());
    }
    async blob() {
      return new g.Blob([await this._consume()], { type: this.headers.get("content-type") ?? "" });
    }
    async formData() {
      const type = this.headers.get("content-type") ?? "";
      const bytes = await this._consume();
      const boundary = type.match(/boundary=(?:"([^"]+)"|([^;]+))/i);
      if (/^multipart\/form-data/i.test(type) && boundary) return parseMultipart(bytes, boundary[1] ?? boundary[2]);
      if (!/^application\/x-www-form-urlencoded/i.test(type)) throw new TypeError("Could not parse content as FormData");
      const form = new g.FormData();
      for (const [k, v] of new g.URLSearchParams(fromUtf8(bytes))) form.append(k, v);
      return form;
    }
    _cloneBody() {
      if (this._used) throw new TypeError("Body has already been consumed");
      if (!this._stream) return this._bytes ? this._bytes.slice() : null;
      const [a, b] = teeStream(this._stream);
      this._stream = a;
      return b;
    }
  }

  /** Two streams with the chunks of one; the source is read as either side pulls. */
  const teeStream = (source) => {
    const reader = source.getReader();
    const queues = [[], []];
    const controllers = [];
    let reading = null;
    const pull = (side) => {
      if (queues[side].length) {
        controllers[side].enqueue(queues[side].shift());
        return;
      }
      reading ??= reader.read().then(({ done, value }) => {
        reading = null;
        for (const i of [0, 1]) {
          if (done) controllers[i].close();
          else if (i === side) controllers[i].enqueue(value);
          else queues[i].push(value);
        }
      });
      return reading;
    };
    const branch = (side) =>
      new g.ReadableStream({
        start(controller) {
          controllers[side] = controller;
        },
        pull: () => pull(side),
      });
    return [branch(0), branch(1)];
  };

  const METHODS = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH"];

  class Request extends Body {
    constructor(input, init = {}) {
      super();
      const source = input instanceof Request ? input : null;
      this.url = source ? source.url : new g.URL(String(input)).href;
      const method = String(init.method ?? source?.method ?? "GET");
      this.method = METHODS.includes(method.toUpperCase()) ? method.toUpperCase() : method;
      this.headers = new g.Headers(init.headers ?? source?.headers);
      this.redirect = init.redirect ?? source?.redirect ?? "follow";
      this.signal = init.signal ?? source?.signal ?? new g.AbortController().signal;
      this.cache = init.cache ?? source?.cache ?? "default";
      this.credentials = init.credentials ?? source?.credentials ?? "same-origin";
      this.mode = init.mode ?? source?.mode ?? "cors";
      this.referrer = init.referrer ?? source?.referrer ?? "about:client";
      this.integrity = init.integrity ?? "";
      this.keepalive = init.keepalive ?? false;
      this.duplex = init.duplex ?? "half";
      const body = "body" in init ? init.body : source ? source._cloneBody() : null;
      if (body != null && (this.method === "GET" || this.method === "HEAD")) {
        throw new TypeError("Request with GET/HEAD method cannot have body");
      }
      this._init(body, this.headers);
    }
    clone() {
      return new Request(this);
    }
  }
  g.Request ??= Request;

  const REDIRECTS = [301, 302, 303, 307, 308];

  class Response extends Body {
    constructor(body = null, init = {}) {
      super();
      const status = init.status ?? 200;
      if (!Number.isInteger(status) || status < 200 || status > 599) {
        throw new RangeError(`Response status ${status} is not in the range 200 to 599`);
      }
      this.status = status;
      this.statusText = String(init.statusText ?? "");
      this.headers = new g.Headers(init.headers);
      this.type = "default";
      this.url = init.url ?? "";
      this.redirected = init.redirected ?? false;
      if (body != null && [101, 204, 205, 304].includes(status)) {
        throw new TypeError(`Response with status ${status} cannot have a body`);
      }
      this._init(body, this.headers);
    }
    get ok() {
      return this.status >= 200 && this.status < 300;
    }
    clone() {
      const response = new this.constructor(this._cloneBody(), this);
      Object.assign(response, { type: this.type, url: this.url, redirected: this.redirected });
      return response;
    }
    static json(data, init = {}) {
      const text = JSON.stringify(data);
      if (text === undefined) throw new TypeError("Value is not JSON serializable");
      const headers = new g.Headers(init.headers);
      if (!headers.has("content-type")) headers.set("content-type", "application/json");
      return new this(text, { ...init, headers });
    }
    static redirect(url, status = 302) {
      if (!REDIRECTS.includes(status)) throw new RangeError(`Invalid redirect status ${status}`);
      const location = url instanceof g.URL ? url.href : String(url);
      return new this(null, { status, headers: { location } });
    }
    static error() {
      const response = new this(null, { status: 200 });
      response.status = 0;
      response.type = "error";
      return response;
    }
  }
  g.Response ??= Response;

  const pending = new Map();
  const abortError = (signal) =>
    signal.reason ?? Object.assign(new Error("This operation was aborted"), { name: "AbortError" });

  /** `fetch` backed by the Rust HTTP client; `[server] fetch_allow` decides which hosts it reaches. */
  g.fetch ??= async (input, init) => {
    const request = new g.Request(input, init);
    if (request.signal.aborted) throw abortError(request.signal);
    const body = request._stream || request._bytes ? await request._consume() : null;
    const head = { url: request.url, method: request.method, headers: request.headers._, redirect: request.redirect };
    return new Promise((resolve, reject) => {
      const id = host.fetch(JSON.stringify(head), body);
      const onAbort = () => {
        pending.delete(id);
        reject(abortError(request.signal));
      };
      request.signal.addEventListener("abort", onAbort, { once: true });
      pending.set(id, {
        resolve,
        reject,
        done: () => request.signal.removeEventListener("abort", onAbort),
      });
    });
  };
  /** Called by the worker when the response of `fetch` request `id` arrived, or the request failed. */
  g.__volt_fetched = (id, head, body, error) => {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    entry.done();
    if (error !== null) return entry.reject(new TypeError(error));
    const meta = JSON.parse(head);
    const empty = [101, 204, 205, 304].includes(meta.status);
    const response = new g.Response(empty ? null : body, {
      status: meta.status < 200 ? 200 : meta.status,
      statusText: meta.statusText,
      headers: meta.headers,
      url: meta.url,
      redirected: meta.redirected,
    });
    response.status = meta.status;
    entry.resolve(response);
  };
  g.process ??= { env: {} };
  g.process.env.NODE_ENV ??= host.env;
})();
