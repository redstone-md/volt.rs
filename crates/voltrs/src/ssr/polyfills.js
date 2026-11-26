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

  const timers = new Map();
  let timerId = 0;
  g.setTimeout = (fn, delay = 0, ...args) => {
    const id = ++timerId;
    timers.set(id, { fn, args, delay: Number(delay) || 0, seq: id });
    return id;
  };
  g.clearTimeout = (id) => void timers.delete(id);
  g.setInterval = g.setTimeout;
  g.clearInterval = g.clearTimeout;
  g.setImmediate = (fn, ...args) => g.setTimeout(fn, 0, ...args);
  g.clearImmediate = g.clearTimeout;
  g.queueMicrotask ??= (fn) => void Promise.resolve().then(fn);
  g.__volt_tick = () => {
    if (timers.size === 0) return 0;
    const due = [...timers.entries()].sort((a, b) => a[1].delay - b[1].delay || a[1].seq - b[1].seq);
    const [id, timer] = due[0];
    timers.delete(id);
    if (typeof timer.fn === "function") timer.fn(...timer.args);
    return 1;
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

  g.TextEncoder ??= class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(input = "") {
      const s = unescape(encodeURIComponent(String(input)));
      const out = new Uint8Array(s.length);
      for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
      return out;
    }
    encodeInto(input, dest) {
      const bytes = this.encode(input);
      dest.set(bytes.subarray(0, dest.length));
      return { read: input.length, written: Math.min(bytes.length, dest.length) };
    }
  };
  g.TextDecoder ??= class TextDecoder {
    get encoding() {
      return "utf-8";
    }
    decode(input) {
      if (!input) return "";
      const bytes = input instanceof Uint8Array ? input : new Uint8Array(input.buffer || input);
      let s = "";
      for (let i = 0; i < bytes.length; i += 8192) s += String.fromCharCode(...bytes.subarray(i, i + 8192));
      try {
        return decodeURIComponent(escape(s));
      } catch {
        return s;
      }
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

  g.AbortController ??= class AbortController {
    constructor() {
      this.signal = { aborted: false, addEventListener() {}, removeEventListener() {} };
    }
    abort() {
      this.signal.aborted = true;
    }
  };
  g.process ??= { env: {} };
  g.process.env.NODE_ENV ??= host.env;
})();
