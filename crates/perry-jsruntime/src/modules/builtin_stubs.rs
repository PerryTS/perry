//! Stub implementations for Node.js built-in modules served via the V8 fallback runtime.

#[allow(unused_imports)]
use super::*;

/// Get a stub implementation for a Node.js built-in module
pub fn get_builtin_stub(name: &str) -> String {
    match name {
        "net" => r#"
// Stub implementation for Node.js 'net' module
export class Socket {
    constructor() {}
    connect() { return this; }
    write() { return true; }
    end() {}
    destroy() {}
    on() { return this; }
    once() { return this; }
    removeListener() { return this; }
    setTimeout() { return this; }
    setNoDelay() { return this; }
    setKeepAlive() { return this; }
}
export class Server {
    constructor() {}
    listen() { return this; }
    close() {}
    on() { return this; }
}
export function createServer() { return new Server(); }
export function createConnection() { return new Socket(); }
export function connect() { return new Socket(); }
export function isIP() { return 0; }
export function isIPv4() { return false; }
export function isIPv6() { return false; }
export default { Socket, Server, createServer, createConnection, connect, isIP, isIPv4, isIPv6 };
"#.to_string(),
        "tls" => r#"
// Stub implementation for Node.js 'tls' module
export class TLSSocket {
    constructor() {}
    connect() { return this; }
    on() { return this; }
}
export function connect() { return new TLSSocket(); }
export function createSecureContext() { return {}; }
export default { TLSSocket, connect, createSecureContext };
"#.to_string(),
        "http" | "https" | "http2" => r#"
// Stub implementation for Node.js http/https/http2 module
//
// `createServer(handler)` bridges to the V8-fallback HTTP server via
// the `op_perry_http_*` ops (see crates/perry-jsruntime/src/ops.rs).
// This is the minimum viable shape — enough for express.listen() to
// receive real requests and respond. NOT a full Node `http.Server`
// (no streams, no trailers, no upgrade events). Apps that need richer
// HTTP server semantics should compile through the native path
// (perry-ext-http-server).
const __perry_core = (typeof Deno !== 'undefined' && Deno.core) ? Deno.core : null;
const __perry_ops = __perry_core && __perry_core.ops ? __perry_core.ops : null;

export class IncomingMessage {
    constructor(opaque) {
        this.method = opaque.method;
        this.url = opaque.url;
        this.headers = opaque.headers || {};
        this.rawHeaders = opaque.rawHeaders || [];
        this.httpVersion = '1.1';
        this.httpVersionMajor = 1;
        this.httpVersionMinor = 1;
        this.complete = true;
        this._body = opaque.body || '';
        this._listeners = Object.create(null);
        // Minimal stream-ish events: synthesize 'data' + 'end' on next tick
        // so handlers that wire `.on('data', ...).on('end', ...)` work.
        Promise.resolve().then(() => {
            const dataCbs = this._listeners['data'] || [];
            if (dataCbs.length > 0 && this._body) {
                for (const cb of dataCbs) cb(this._body);
            }
            const endCbs = this._listeners['end'] || [];
            for (const cb of endCbs) cb();
        });
    }
    on(event, listener) {
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => fn.apply(this, args));
        return !!arr;
    }
    setEncoding() { return this; }
    pause() { return this; }
    resume() { return this; }
}

export class ServerResponse {
    constructor(reqId) {
        this._reqId = reqId;
        this._status = 200;
        this._headers = []; // array of [name, value] preserving order
        this._headerMap = Object.create(null); // lowercase -> last value
        this._body = '';
        this._ended = false;
        this.headersSent = false;
        this.finished = false;
        this.statusCode = 200;
        this.statusMessage = '';
        this._listeners = Object.create(null);
    }
    setHeader(name, value) {
        const lower = String(name).toLowerCase();
        // Replace any previous entry for the same header.
        this._headers = this._headers.filter(p => p[0].toLowerCase() !== lower);
        this._headers.push([String(name), String(value)]);
        this._headerMap[lower] = String(value);
        return this;
    }
    getHeader(name) { return this._headerMap[String(name).toLowerCase()]; }
    getHeaders() {
        const out = {};
        for (const [k, v] of this._headers) out[k.toLowerCase()] = v;
        return out;
    }
    getHeaderNames() { return Object.keys(this._headerMap); }
    hasHeader(name) { return String(name).toLowerCase() in this._headerMap; }
    removeHeader(name) {
        const lower = String(name).toLowerCase();
        this._headers = this._headers.filter(p => p[0].toLowerCase() !== lower);
        delete this._headerMap[lower];
        return this;
    }
    writeHead(status, statusMessageOrHeaders, headers) {
        this._status = status;
        this.statusCode = status;
        let h = headers;
        if (statusMessageOrHeaders && typeof statusMessageOrHeaders !== 'string') {
            h = statusMessageOrHeaders;
        } else if (typeof statusMessageOrHeaders === 'string') {
            this.statusMessage = statusMessageOrHeaders;
        }
        if (h) {
            if (Array.isArray(h)) {
                // [name1, val1, name2, val2, ...]
                for (let i = 0; i + 1 < h.length; i += 2) this.setHeader(h[i], h[i + 1]);
            } else {
                for (const k of Object.keys(h)) this.setHeader(k, h[k]);
            }
        }
        return this;
    }
    write(chunk) {
        if (this._ended) return false;
        if (chunk == null) return true;
        if (typeof chunk === 'string') {
            this._body += chunk;
        } else if (chunk instanceof Uint8Array) {
            this._body += new TextDecoder().decode(chunk);
        } else if (chunk && chunk.buffer instanceof ArrayBuffer) {
            this._body += new TextDecoder().decode(new Uint8Array(chunk.buffer));
        } else {
            this._body += String(chunk);
        }
        return true;
    }
    end(chunk, encoding, cb) {
        if (this._ended) return this;
        if (typeof chunk === 'function') { cb = chunk; chunk = undefined; }
        else if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
        if (chunk != null) this.write(chunk);
        this._ended = true;
        this.finished = true;
        this.headersSent = true;
        // Default Content-Length when caller hasn't set Transfer-Encoding.
        const lower = this._headerMap;
        if (!lower['content-length'] && !lower['transfer-encoding']) {
            const len = (typeof TextEncoder !== 'undefined')
                ? new TextEncoder().encode(this._body).length
                : this._body.length;
            this.setHeader('Content-Length', String(len));
        }
        if (__perry_ops && __perry_ops.op_perry_http_respond) {
            try {
                __perry_ops.op_perry_http_respond(
                    this._reqId, this._status, JSON.stringify(this._headers), this._body);
            } catch (_) {}
        }
        const finishCbs = this._listeners['finish'] || [];
        for (const f of finishCbs) { try { f(); } catch (_) {} }
        const closeCbs = this._listeners['close'] || [];
        for (const f of closeCbs) { try { f(); } catch (_) {} }
        if (typeof cb === 'function') { try { cb(); } catch (_) {} }
        return this;
    }
    on(event, listener) {
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => fn.apply(this, args));
        return !!arr;
    }
    flushHeaders() { this.headersSent = true; }
}

export class Agent {}

class Server {
    constructor(handler) {
        this._handler = handler || (() => {});
        this._serverId = 0;
        this._listening = false;
        this._listeners = Object.create(null);
        this._listenAddress = null;
    }
    _emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => { try { fn.apply(this, args); } catch (_) {} });
    }
    on(event, listener) {
        if (event === 'request' && typeof listener === 'function') {
            // Mirror Node: 'request' listeners run in addition to the
            // constructor handler. Stash them in _listeners and dispatch
            // alongside the main handler in the accept loop.
        }
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    addListener(event, listener) { return this.on(event, listener); }
    emit(event, ...args) { this._emit(event, ...args); return true; }
    setTimeout() { return this; }
    address() {
        if (!this._listenAddress) return null;
        return { port: this._listenAddress.port, address: this._listenAddress.host, family: 'IPv4' };
    }
    listen(...args) {
        // express calls listen(port, cb); also support (port, host, cb) and ({port, host}, cb).
        let port = 3000;
        let host = '0.0.0.0';
        let cb = null;
        for (const a of args) {
            if (typeof a === 'number') port = a;
            else if (typeof a === 'string') {
                const n = parseInt(a, 10);
                if (!isNaN(n)) port = n;
            } else if (typeof a === 'function') cb = a;
            else if (a && typeof a === 'object') {
                if (typeof a.port === 'number') port = a.port;
                if (typeof a.host === 'string') host = a.host;
            }
        }
        if (!__perry_ops || !__perry_ops.op_perry_http_listen) {
            throw new Error('http.createServer: Perry V8-fallback HTTP ops unavailable');
        }
        const self = this;
        // Kick off the bind + accept loop. Once bound, fire 'listening'
        // + the user callback and begin dispatching to the handler.
        //
        // Critical ordering for the express + native-fetch self-call
        // pattern (#997): the listening callback is invoked AFTER the
        // accept loop has registered its first `op_perry_http_accept`
        // op. This matters because user callbacks compiled from Perry
        // TypeScript come in as native trampolines — the V8 thread is
        // blocked synchronously while the callback's native body runs,
        // and any `await` inside the body busy-waits on the V8 thread
        // (pumping `js_run_jsruntime_pump` inside the wait so microtasks
        // still progress). If we called `cb()` BEFORE the accept loop
        // queued its op, an `await fetch('http://127.0.0.1:port/...')`
        // inside cb would block waiting for the server to respond, but
        // the server can't dispatch because the accept loop hasn't
        // started — three-way deadlock against the synchronous trampoline.
        //
        // Scheduling cb on Promise.resolve().then() defers it past the
        // first `await op_perry_http_accept(...)`, so by the time the
        // trampoline starts blocking the V8 thread there's already a
        // pending accept op the inner pump can drive to resolution
        // when hyper hands a request through the mpsc.
        (async () => {
            try {
                const sid = await __perry_ops.op_perry_http_listen(port, host);
                self._serverId = sid;
                self._listening = true;
                self._listenAddress = { port, host };
                self._emit('listening');
                if (typeof cb === 'function') {
                    // Defer to a microtask so the accept loop below
                    // registers `op_perry_http_accept` first.
                    Promise.resolve().then(() => { try { cb(); } catch (_) {} });
                }
                // Accept loop
                while (self._listening && self._serverId !== 0) {
                    let r;
                    try { r = await __perry_ops.op_perry_http_accept(sid); }
                    catch (_) { break; }
                    if (!r || r.id === 0) break;
                    const req = new IncomingMessage(r);
                    const res = new ServerResponse(r.id);
                    // Fire 'request' listeners + the constructor handler.
                    const reqListeners = self._listeners['request'] || [];
                    for (const fn of reqListeners) {
                        try { fn.call(self, req, res); } catch (e) { /* swallow */ }
                    }
                    try {
                        const ret = self._handler(req, res);
                        if (ret && typeof ret.then === 'function') {
                            ret.catch(() => {});
                        }
                    } catch (e) {
                        if (!res._ended) {
                            try {
                                res.statusCode = 500;
                                res.end('Internal Server Error');
                            } catch (_) {}
                        }
                    }
                }
            } catch (err) {
                self._emit('error', err);
                if (typeof cb === 'function') { try { cb(err); } catch (_) {} }
            }
        })();
        return this;
    }
    close(cb) {
        if (this._serverId && __perry_ops && __perry_ops.op_perry_http_close) {
            try { __perry_ops.op_perry_http_close(this._serverId); } catch (_) {}
        }
        this._listening = false;
        this._serverId = 0;
        this._emit('close');
        if (typeof cb === 'function') { try { cb(); } catch (_) {} }
        return this;
    }
    closeAllConnections() {}
    closeIdleConnections() {}
    get listening() { return this._listening; }
}

// Issue #912 (#909 follow-up): express/router read `const { METHODS } =
// require('node:http')` at module init and immediately call `METHODS.map(...)`.
export const METHODS = [
    'ACL', 'BIND', 'CHECKOUT', 'CONNECT', 'COPY', 'DELETE', 'GET', 'HEAD',
    'LINK', 'LOCK', 'M-SEARCH', 'MERGE', 'MKACTIVITY', 'MKCALENDAR', 'MKCOL',
    'MOVE', 'NOTIFY', 'OPTIONS', 'PATCH', 'POST', 'PROPFIND', 'PROPPATCH',
    'PURGE', 'PUT', 'QUERY', 'REBIND', 'REPORT', 'SEARCH', 'SOURCE',
    'SUBSCRIBE', 'TRACE', 'UNBIND', 'UNLINK', 'UNLOCK', 'UNSUBSCRIBE'
];
export const STATUS_CODES = {
    100: 'Continue', 101: 'Switching Protocols', 200: 'OK', 201: 'Created',
    202: 'Accepted', 204: 'No Content', 301: 'Moved Permanently',
    302: 'Found', 304: 'Not Modified', 400: 'Bad Request', 401: 'Unauthorized',
    403: 'Forbidden', 404: 'Not Found', 405: 'Method Not Allowed',
    408: 'Request Timeout', 409: 'Conflict', 410: 'Gone', 413: 'Payload Too Large',
    414: 'URI Too Long', 415: 'Unsupported Media Type', 429: 'Too Many Requests',
    500: 'Internal Server Error', 501: 'Not Implemented', 502: 'Bad Gateway',
    503: 'Service Unavailable', 504: 'Gateway Timeout'
};
export function request() { throw new Error('http.request not supported in this environment'); }
export function get() { throw new Error('http.get not supported in this environment'); }
export function createServer(handler) { return new Server(handler); }
export function createSecureServer() { throw new Error('http2.createSecureServer not supported in this environment'); }
export { Server };
export default { IncomingMessage, ServerResponse, Server, Agent, METHODS, STATUS_CODES, request, get, createServer, createSecureServer };
"#.to_string(),
        "crypto" => r#"
// Stub implementation for Node.js 'crypto' module
export function randomBytes(size) {
    const arr = new Uint8Array(size);
    crypto.getRandomValues(arr);
    return arr;
}
// Issue: jose imports `randomFillSync` from `node:crypto` via the V8/JS
// fallback path. Node's `randomFillSync(buffer, offset?, size?)` fills the
// given TypedArray/Buffer with cryptographically secure random bytes in
// place and returns the buffer.
export function randomFillSync(buf, offset, size) {
    const o = offset || 0;
    const len = (buf && typeof buf.length === 'number') ? buf.length : 0;
    const n = (size != null) ? size : (len - o);
    let view;
    if (typeof buf.subarray === 'function') {
        view = buf.subarray(o, o + n);
    } else if (buf && buf.buffer) {
        view = new Uint8Array(buf.buffer, (buf.byteOffset || 0) + o, n);
    } else {
        view = buf;
    }
    crypto.getRandomValues(view);
    return buf;
}
export function randomUUID() {
    // RFC 4122 v4 UUID using getRandomValues
    const b = new Uint8Array(16);
    crypto.getRandomValues(b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const h = [];
    for (let i = 0; i < 16; i++) h.push((b[i] + 0x100).toString(16).slice(1));
    return h[0]+h[1]+h[2]+h[3]+'-'+h[4]+h[5]+'-'+h[6]+h[7]+'-'+h[8]+h[9]+'-'+h[10]+h[11]+h[12]+h[13]+h[14]+h[15];
}
// `crypto.createHash(algorithm)` — real digest for sha1/sha256/sha384/
// sha512/md5 via `op_perry_hash`. Mirrors the createHmac path: accumulate
// `update()` inputs into a single Uint8Array on `digest()` to match
// Node's API shape. Used by NestJS's `ModuleTokenFactory.hashString`
// and `fast-safe-stringify` (`safeStableStringify` token hashing) so
// every module token comes back unique instead of all hashing to
// `""`. (#1021.)
export function createHash(algorithm) {
    const normalize = (a) => {
        if (typeof a !== 'string') return null;
        const x = a.toLowerCase();
        if (x === 'sha1' || x === 'sha-1') return 'sha1';
        if (x === 'sha256' || x === 'sha-256') return 'sha256';
        if (x === 'sha384' || x === 'sha-384') return 'sha384';
        if (x === 'sha512' || x === 'sha-512') return 'sha512';
        if (x === 'md5') return 'md5';
        return null;
    };
    const toBytes = (input) => {
        if (input == null) return new Uint8Array(0);
        if (input instanceof Uint8Array) return input;
        if (input instanceof ArrayBuffer) return new Uint8Array(input);
        if (ArrayBuffer.isView(input)) {
            return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
        }
        if (typeof input === 'string') {
            return new TextEncoder().encode(input);
        }
        return new Uint8Array(0);
    };
    const concat = (chunks) => {
        let total = 0;
        for (const c of chunks) total += c.length;
        const out = new Uint8Array(total);
        let off = 0;
        for (const c of chunks) { out.set(c, off); off += c.length; }
        return out;
    };
    const toHex = (bytes) => {
        let s = '';
        for (let i = 0; i < bytes.length; i++) {
            s += (bytes[i] + 0x100).toString(16).slice(1);
        }
        return s;
    };
    const toBase64 = (bytes) => {
        if (typeof Buffer !== 'undefined' && Buffer.from) {
            return Buffer.from(bytes).toString('base64');
        }
        let s = '';
        for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
        return (typeof btoa === 'function') ? btoa(s) : '';
    };
    const alg = normalize(algorithm);
    const ops = (typeof Deno !== 'undefined' && Deno.core && Deno.core.ops) ? Deno.core.ops : null;
    const chunks = [];
    let finalized = false;
    return {
        update(data, _enc) {
            if (finalized) {
                throw new Error('Digest already called');
            }
            chunks.push(toBytes(data));
            return this;
        },
        digest(encoding) {
            finalized = true;
            const merged = concat(chunks);
            let out;
            if (alg && ops && typeof ops.op_perry_hash === 'function') {
                out = ops.op_perry_hash(alg, merged);
                if (!(out instanceof Uint8Array)) out = new Uint8Array(out || []);
            } else {
                out = new Uint8Array(0);
            }
            if (!encoding || encoding === 'binary') {
                if (typeof Buffer !== 'undefined' && Buffer.from) {
                    return Buffer.from(out);
                }
                return out;
            }
            if (encoding === 'hex') return toHex(out);
            if (encoding === 'base64') return toBase64(out);
            if (encoding === 'base64url') {
                return toBase64(out)
                    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
            }
            return toHex(out);
        },
        copy() {
            if (finalized) throw new Error('Digest already called');
            const clone = createHash(algorithm);
            for (const c of chunks) clone.update(c);
            return clone;
        },
    };
}
// `crypto.createHmac(algorithm, key)` — real HMAC for HS256/384/512 via
// `op_perry_hmac`. The op takes a normalized algorithm name plus
// zero-copy Uint8Array buffers for the key/data. We accumulate update()
// inputs into a single Uint8Array on .digest() to match Node's API
// shape (Node's createHmac returns a Hmac that supports chained
// update().update().digest()). Without this, libraries like `jose`
// fall through to an empty signature and produce malformed JWS tokens.
const __perry_hmac_normalize_alg = (algorithm) => {
    if (typeof algorithm !== 'string') return null;
    const a = algorithm.toLowerCase();
    if (a === 'sha256' || a === 'sha-256') return 'sha256';
    if (a === 'sha384' || a === 'sha-384') return 'sha384';
    if (a === 'sha512' || a === 'sha-512') return 'sha512';
    return null;
};
const __perry_hmac_to_bytes = (input) => {
    if (input == null) return new Uint8Array(0);
    if (input instanceof Uint8Array) return input;
    if (input instanceof ArrayBuffer) return new Uint8Array(input);
    if (ArrayBuffer.isView(input)) {
        return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    }
    if (typeof input === 'string') {
        return new TextEncoder().encode(input);
    }
    // KeyObject / shim with .export(), ._material, or ._key — best-effort
    // unwrap. Check _material first so we don't recurse through the Buffer
    // returned from export() (which would already be a Uint8Array hit).
    if (input._material instanceof Uint8Array) return input._material;
    if (typeof input.export === 'function') {
        try { return __perry_hmac_to_bytes(input.export()); } catch (_) {}
    }
    if (input._key != null) return __perry_hmac_to_bytes(input._key);
    return new Uint8Array(0);
};
const __perry_hmac_concat = (chunks) => {
    let total = 0;
    for (const c of chunks) total += c.length;
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
};
const __perry_hmac_to_hex = (bytes) => {
    let s = '';
    for (let i = 0; i < bytes.length; i++) {
        s += (bytes[i] + 0x100).toString(16).slice(1);
    }
    return s;
};
const __perry_hmac_to_base64 = (bytes) => {
    if (typeof Buffer !== 'undefined' && Buffer.from) {
        return Buffer.from(bytes).toString('base64');
    }
    // Fallback (no Buffer): manual base64.
    let bin = '';
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    if (typeof btoa === 'function') return btoa(bin);
    return bin;
};
export function createHmac(algorithm, key) {
    const alg = __perry_hmac_normalize_alg(algorithm);
    const keyBytes = __perry_hmac_to_bytes(key);
    const chunks = [];
    const ops = (typeof Deno !== 'undefined' && Deno.core && Deno.core.ops) ? Deno.core.ops : null;
    return {
        update(data, _inputEncoding) {
            chunks.push(__perry_hmac_to_bytes(data));
            return this;
        },
        digest(encoding) {
            const dataBytes = __perry_hmac_concat(chunks);
            let out;
            if (alg && ops && typeof ops.op_perry_hmac === 'function') {
                out = ops.op_perry_hmac(alg, keyBytes, dataBytes);
                if (!(out instanceof Uint8Array)) out = new Uint8Array(out || []);
            } else {
                out = new Uint8Array(0);
            }
            if (encoding === 'hex') return __perry_hmac_to_hex(out);
            if (encoding === 'base64') return __perry_hmac_to_base64(out);
            if (encoding === 'base64url') {
                return __perry_hmac_to_base64(out).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
            }
            // No encoding → Buffer/Uint8Array. Prefer Node-style Buffer
            // (jose calls `.toString('base64url')` on the result). Buffer
            // extends Uint8Array, so `instanceof Uint8Array` checks still
            // pass on the returned value.
            if (typeof Buffer !== 'undefined' && Buffer.from) {
                return Buffer.from(out);
            }
            return out;
        }
    };
}
export function pbkdf2Sync() { return new Uint8Array(32); }
export function pbkdf2() { return Promise.resolve(new Uint8Array(32)); }
// Best-effort stubs for additional `node:crypto` named exports that
// libraries like `jose` import at module-init time. Returning sane
// no-ops lets the import resolve (so ESM linking succeeds and the
// library can load), even when the underlying primitive isn't wired
// through Perry's native crypto path. Calling them at runtime in the
// V8 fallback path will throw or no-op deterministically, NOT crash
// the JS module loader. Real implementations live behind the native
// FFI path (`Expr::Crypto*` in HIR / `js_crypto_*` in perry-stdlib).
export function timingSafeEqual(a, b) {
    if (!a || !b || a.length !== b.length) return false;
    let r = 0;
    for (let i = 0; i < a.length; i++) r |= a[i] ^ b[i];
    return r === 0;
}
export function createCipheriv() { throw new Error('crypto.createCipheriv not supported in V8 fallback'); }
export function createDecipheriv() { throw new Error('crypto.createDecipheriv not supported in V8 fallback'); }
// `KeyObject` (V8 fallback shim) — must be a real class so that
// `key instanceof KeyObject` checks in libraries like `jose` succeed.
// `createSecretKey(key, encoding)` returns a KeyObject; for utf8 input
// we encode via TextEncoder so the underlying bytes match what HMAC
// expects (Node's `createSecretKey('secret', 'utf8')` byte-aligns
// with Buffer.from('secret', 'utf8')).
export class KeyObject {
    constructor(opts) {
        opts = opts || {};
        this.type = opts.type || 'secret';
        this.asymmetricKeyType = opts.asymmetricKeyType;
        this.asymmetricKeyDetails = opts.asymmetricKeyDetails;
        this._material = opts._material || new Uint8Array(0);
        this.symmetricKeySize = this._material.length;
    }
    export(options) {
        // Node returns a Buffer for secret keys when no format is given.
        if (typeof Buffer !== 'undefined' && Buffer.from) {
            return Buffer.from(this._material);
        }
        return this._material;
    }
    static from(cryptoKey) {
        return new KeyObject({ type: 'secret', _material: new Uint8Array(0) });
    }
}
export function createSecretKey(key, encoding) {
    let bytes;
    if (key instanceof Uint8Array) {
        bytes = key;
    } else if (typeof key === 'string') {
        // Node accepts a string + encoding; only utf8 is common.
        if (encoding && encoding !== 'utf8' && encoding !== 'utf-8') {
            // Best-effort: hex / base64 decoders are not free here. Fall
            // back to utf8 bytes so the caller still gets a valid key.
        }
        bytes = new TextEncoder().encode(key);
    } else if (key && key.buffer instanceof ArrayBuffer) {
        bytes = new Uint8Array(key.buffer, key.byteOffset || 0, key.byteLength);
    } else {
        bytes = new Uint8Array(0);
    }
    return new KeyObject({ type: 'secret', _material: bytes });
}
export function createPrivateKey() { throw new Error('crypto.createPrivateKey not supported in V8 fallback'); }
export function createPublicKey() { throw new Error('crypto.createPublicKey not supported in V8 fallback'); }
export function generateKeyPair() { throw new Error('crypto.generateKeyPair not supported in V8 fallback'); }
export function diffieHellman() { throw new Error('crypto.diffieHellman not supported in V8 fallback'); }
export function publicEncrypt() { throw new Error('crypto.publicEncrypt not supported in V8 fallback'); }
export function privateDecrypt() { throw new Error('crypto.privateDecrypt not supported in V8 fallback'); }
export function getCiphers() { return []; }
export const constants = {
    RSA_PKCS1_PADDING: 1,
    RSA_PKCS1_OAEP_PADDING: 4,
    RSA_PSS_SALTLEN_DIGEST: -1,
    RSA_PSS_SALTLEN_MAX_SIGN: -2,
    RSA_PSS_SALTLEN_AUTO: -2,
};
// `crypto.webcrypto` — Node exposes the Web Crypto API namespace here.
// jose's `webcrypto.js` reads `crypto.webcrypto.CryptoKey`; returning a
// minimal object with the same surface lets ESM linking succeed and
// makes `crypto.webcrypto?.CryptoKey` resolve to `undefined` rather
// than crash with a TypeError.
export const webcrypto = (typeof globalThis !== 'undefined' && globalThis.crypto)
    ? globalThis.crypto
    : { subtle: {}, getRandomValues(arr) { return arr; } };
// `crypto.sign(algorithm, data, key, callback)` — Node's one-shot
// signer. jose's `runtime/sign.js` does `promisify(crypto.sign)` and
// only calls the resulting Promise variant for non-HS algorithms.
// Throwing here keeps the V8 stub deterministic: HS* never touches
// `crypto.sign`, and asymmetric signing is unsupported on this path.
export function sign(algorithm, data, key, callback) {
    const err = new Error('crypto.sign (asymmetric) not supported in V8 fallback');
    if (typeof callback === 'function') {
        callback(err);
        return;
    }
    throw err;
}
export function verify() { throw new Error('crypto.verify (asymmetric) not supported in V8 fallback'); }
export default { randomBytes, randomFillSync, randomUUID, createHash, createHmac, pbkdf2Sync, pbkdf2, timingSafeEqual, createCipheriv, createDecipheriv, createSecretKey, createPrivateKey, createPublicKey, generateKeyPair, diffieHellman, publicEncrypt, privateDecrypt, getCiphers, KeyObject, constants, webcrypto, sign, verify };
"#.to_string(),
        "fs" => r#"
// Stub implementation for Node.js 'fs' module
export function readFileSync() { throw new Error('fs.readFileSync not supported'); }
export function writeFileSync() { throw new Error('fs.writeFileSync not supported'); }
export function existsSync() { return false; }
export function mkdirSync() {}
export function readdirSync() { return []; }
export function statSync() { throw new Error('fs.statSync not supported'); }
export function isDirectory() { return 0; }
export const promises = {
    readFile: async () => { throw new Error('fs.promises.readFile not supported'); },
    writeFile: async () => { throw new Error('fs.promises.writeFile not supported'); },
};
export default { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync, statSync, isDirectory, promises };
"#.to_string(),
        "path" => r#"
// Stub implementation for Node.js 'path' module
export const sep = '/';
export const delimiter = ':';
export function join(...parts) { return parts.join('/').replace(/\/+/g, '/'); }
export function resolve(...parts) { return '/' + parts.join('/').replace(/\/+/g, '/'); }
export function dirname(p) { return p.split('/').slice(0, -1).join('/') || '/'; }
export function basename(p, ext) {
    let base = p.split('/').pop() || '';
    if (ext && base.endsWith(ext)) base = base.slice(0, -ext.length);
    return base;
}
export function extname(p) { const m = p.match(/\.[^.]+$/); return m ? m[0] : ''; }
export function isAbsolute(p) { return p.startsWith('/'); }
export function normalize(p) { return p.replace(/\/+/g, '/'); }
export function relative(from, to) { return to; }
export function parse(p) { return { root: '/', dir: dirname(p), base: basename(p), ext: extname(p), name: basename(p, extname(p)) }; }
export function format(obj) { return (obj.dir || '') + '/' + (obj.base || obj.name + obj.ext); }
export default { sep, delimiter, join, resolve, dirname, basename, extname, isAbsolute, normalize, relative, parse, format };
"#.to_string(),
        "os" => r#"
// Stub implementation for Node.js 'os' module
export function platform() { return 'unknown'; }
export function arch() { return 'unknown'; }
export function cpus() { return []; }
export function homedir() { return '/'; }
export function tmpdir() { return '/tmp'; }
export function hostname() { return 'localhost'; }
export function type() { return 'Unknown'; }
export function release() { return '0.0.0'; }
export function totalmem() { return 0; }
export function freemem() { return 0; }
export function uptime() { return 0; }
export function loadavg() { return [0, 0, 0]; }
export function networkInterfaces() { return {}; }
export const EOL = '\n';
export default { platform, arch, cpus, homedir, tmpdir, hostname, type, release, totalmem, freemem, uptime, loadavg, networkInterfaces, EOL };
"#.to_string(),
        "stream" => r#"
// Stub implementation for Node.js 'stream' module.
//
// IMPORTANT: Node's `require('stream')` returns the legacy `Stream`
// *constructor* (a class) with `Readable`/`Writable`/etc. attached as
// static properties — NOT a plain namespace object. Packages like
// `send` / `express` rely on this shape:
//
//     var Stream = require('stream')
//     function SendStream() { Stream.call(this) }
//     util.inherits(SendStream, Stream)   // reads Stream.prototype
//
// If the default export is `{ Readable, Writable, ... }` then
// `Stream.prototype` is `undefined` and `util.inherits` blows up with
// "Object prototype may only be an Object or null: undefined".
// (See: node_modules/send/index.js:30,173.)
//
// So we make `Stream` a real class, attach the sub-classes as static
// properties, and export the *class itself* as default.
class Stream {
    constructor() {}
    pipe(dest) { return dest; }
    on() { return this; }
    once() { return this; }
    emit() { return false; }
    off() { return this; }
    addListener() { return this; }
    removeListener() { return this; }
    removeAllListeners() { return this; }
}
export class Readable extends Stream {
    constructor() { super(); }
    read() { return null; }
    pipe(dest) { return dest; }
}
export class Writable extends Stream {
    constructor() { super(); }
    write() { return true; }
    end() {}
}
export class Duplex extends Readable {
    write() { return true; }
    end() {}
}
export class Transform extends Duplex {}
export class PassThrough extends Transform {}
export class ReadableStream {}
export class WritableStream {}
export class TransformStream {}
export function pipeline() {}
export function finished() {}
// Attach sub-classes as static properties so `Stream.Readable`,
// `Stream.Writable`, etc. resolve the way Node ships them.
Stream.Readable = Readable;
Stream.Writable = Writable;
Stream.Duplex = Duplex;
Stream.Transform = Transform;
Stream.PassThrough = PassThrough;
Stream.pipeline = pipeline;
Stream.finished = finished;
export { Stream };
// `__perry_commonjs = true` tells the wrap_commonjs() require() shim in
// modules.rs to return `module.default` instead of the ESM namespace
// when this module is `require()`'d. Node's `require('stream')` returns
// the Stream class itself (with `.Readable` / `.prototype` / etc),
// NOT a namespace object. Without this flag, `var Stream =
// require('stream')` ends up as a copied null-proto object and
// `Stream.prototype` becomes undefined → `util.inherits` crashes.
export const __perry_commonjs = true;
export default Stream;
"#.to_string(),
        "repl" => r#"
// Stub implementation for Node.js 'repl' module
export function start() {
    return {
        context: {},
        on() { return this; },
        close() {}
    };
}
export default { start };
"#.to_string(),
        "timers" => r#"
// Stub implementation for Node.js 'timers' module
export const setTimeout = globalThis.setTimeout.bind(globalThis);
export const clearTimeout = globalThis.clearTimeout.bind(globalThis);
export const setInterval = globalThis.setInterval.bind(globalThis);
export const clearInterval = globalThis.clearInterval.bind(globalThis);
export const setImmediate = globalThis.setImmediate || ((fn, ...args) => setTimeout(fn, 0, ...args));
export const clearImmediate = globalThis.clearImmediate || clearTimeout;
export default { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate };
"#.to_string(),
        "buffer" => r#"
// Stub implementation for Node.js 'buffer' module
export const Buffer = globalThis.Buffer || {
    from: (data, encoding) => new Uint8Array(typeof data === 'string' ? new TextEncoder().encode(data) : data),
    alloc: (size) => new Uint8Array(size),
    allocUnsafe: (size) => new Uint8Array(size),
    isBuffer: (obj) => obj instanceof Uint8Array,
    concat: (list) => {
        const total = list.reduce((acc, arr) => acc + arr.length, 0);
        const result = new Uint8Array(total);
        let offset = 0;
        for (const arr of list) { result.set(arr, offset); offset += arr.length; }
        return result;
    },
};
// Node's buffer.constants — pino / thread-stream read MAX_STRING_LENGTH at
// module init time (`const MAX_STRING = buffer.constants.MAX_STRING_LENGTH`).
// Without this, the V8-fallback evaluation throws TypeError at top-level
// and the whole module namespace is lost — surfaces as
// `[js_get_export] failed to get namespace: ...MAX_STRING_LENGTH`.
// Values mirror Node 20+: MAX_LENGTH = 2^53-1, MAX_STRING_LENGTH = 2^29-24.
export const constants = {
    MAX_LENGTH: 9007199254740991,
    MAX_STRING_LENGTH: 536870888,
};
export const kMaxLength = constants.MAX_LENGTH;
export const kStringMaxLength = constants.MAX_STRING_LENGTH;
export default { Buffer, constants, kMaxLength, kStringMaxLength };
"#.to_string(),
        "util" => r#"
// Stub implementation for Node.js 'util' module
export function promisify(fn) { return (...args) => new Promise((resolve, reject) => fn(...args, (err, result) => err ? reject(err) : resolve(result))); }
export function callbackify(fn) { return (...args) => { const cb = args.pop(); fn(...args).then(r => cb(null, r)).catch(cb); }; }
export function inspect(obj) { return JSON.stringify(obj); }
export function format(fmt, ...args) { return fmt; }
// util.formatWithOptions(inspectOptions, format[, ...args]) — identical to
// util.format with the first arg routed into util.inspect for %o/%O. Our
// stub ignores the options object and delegates to format(); full
// options-passthrough is a follow-up. Required by the `debug` npm package.
export function formatWithOptions(_inspectOptions, fmt, ...args) { return format(fmt, ...args); }
export function debuglog() { return () => {}; }
export function deprecate(fn) { return fn; }
// `util.inherits(ctor, superCtor)` — Node's pre-class inheritance helper.
// Real Node semantics:
//   Object.defineProperty(ctor, 'super_', { value: superCtor });
//   Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
// Throws TypeError if either arg is missing a `.prototype`. We mirror that
// contract so packages like `send` (which derives `SendStream` from
// `require('stream')`) work transparently.
export function inherits(ctor, superCtor) {
    if (ctor === undefined || ctor === null) {
        throw new TypeError('The constructor to "inherits" must not be null or undefined');
    }
    if (superCtor === undefined || superCtor === null) {
        throw new TypeError('The super constructor to "inherits" must not be null or undefined');
    }
    if (superCtor.prototype === undefined) {
        throw new TypeError('The super constructor to "inherits" must have a prototype');
    }
    Object.defineProperty(ctor, 'super_', { value: superCtor, writable: true, configurable: true });
    Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
}
export const TextEncoder = globalThis.TextEncoder;
export const TextDecoder = globalThis.TextDecoder;
// util.types — Node's runtime introspection namespace. NestJS / rxjs
// reach into this for cheap Promise / TypedArray / Map / Set probes
// during DI dispatch. Most call sites just want a boolean; returning
// `false` for an unknown shape is the conservative answer (the caller
// then falls through to its own duck-typing path).
const _isPromiseLike = (v) => v != null && (typeof v === "object" || typeof v === "function") && typeof v.then === "function";
// `_isKeyObjectShape` — duck-types the `node:crypto` `KeyObject` shim
// without importing it (avoids a circular module dependency between
// node:util and node:crypto). Real Node uses an internal class brand;
// for our V8 fallback path the shape check is sufficient because
// libraries (jose) only ever produce KeyObjects via `createSecretKey`
// or `KeyObject.from`, both of which set `type: 'secret' | ...`
// alongside a typed-array `_material` field.
const _isKeyObjectShape = (v) => (
    v != null && typeof v === 'object'
    && typeof v.type === 'string'
    && (v._material instanceof Uint8Array || v._key != null
        || typeof v.export === 'function')
);
const _isCryptoKeyShape = (v) => (
    v != null && typeof v === 'object'
    && typeof v.algorithm === 'object'
    && typeof v.type === 'string'
    && typeof v.extractable === 'boolean'
);
export const types = {
    isPromise: (v) => _isPromiseLike(v),
    isAsyncFunction: (v) => typeof v === "function" && v.constructor && v.constructor.name === "AsyncFunction",
    isGeneratorFunction: (v) => typeof v === "function" && v.constructor && v.constructor.name === "GeneratorFunction",
    isMap: (v) => v instanceof Map,
    isSet: (v) => v instanceof Set,
    isWeakMap: (v) => v instanceof WeakMap,
    isWeakSet: (v) => v instanceof WeakSet,
    isRegExp: (v) => v instanceof RegExp,
    isDate: (v) => v instanceof Date,
    isArrayBuffer: (v) => v instanceof ArrayBuffer,
    isSharedArrayBuffer: () => false,
    isDataView: (v) => v instanceof DataView,
    isUint8Array: (v) => v instanceof Uint8Array,
    isTypedArray: (v) => ArrayBuffer.isView(v) && !(v instanceof DataView),
    isProxy: () => false,
    isNativeError: (v) => v instanceof Error,
    isBoxedPrimitive: () => false,
    isAnyArrayBuffer: (v) => v instanceof ArrayBuffer,
    isModuleNamespaceObject: () => false,
    // `util.types.isKeyObject` / `isCryptoKey` are required by `jose`
    // (and other JWT/JOSE libs) to discriminate between Uint8Array,
    // CryptoKey, and Node's KeyObject before signing/verifying. The
    // V8 fallback path doesn't expose a real `internalBinding('util')`
    // so we duck-type against the shape produced by our crypto shim.
    isKeyObject: (v) => _isKeyObjectShape(v),
    isCryptoKey: (v) => _isCryptoKeyShape(v),
};
export default { promisify, callbackify, inspect, format, formatWithOptions, debuglog, deprecate, inherits, TextEncoder, TextDecoder, types };
"#.to_string(),
        "events" => r#"
// Stub implementation for Node.js 'events' module.
// Every method lazy-initializes `_events` so mixin/inherit patterns that
// copy EventEmitter.prototype without invoking the constructor (e.g.
// express's createApplication -> mixin(app, EventEmitter.prototype, false))
// still work. This mirrors Node's real lib/events.js, which does
// `this._events ??= ObjectCreate(null)` inside every method.
function __perry_ee_init(self) { if (!self._events) self._events = Object.create(null); return self._events; }
export class EventEmitter {
    constructor() { this._events = Object.create(null); }
    on(event, listener) { const e = __perry_ee_init(this); (e[event] = e[event] || []).push(listener); return this; }
    addListener(event, listener) { return this.on(event, listener); }
    once(event, listener) { __perry_ee_init(this); const wrapped = (...args) => { this.off(event, wrapped); listener(...args); }; return this.on(event, wrapped); }
    off(event, listener) { const e = __perry_ee_init(this); const arr = e[event]; if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); } return this; }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) { const e = __perry_ee_init(this); const arr = e[event]; if (arr) arr.slice().forEach(fn => fn.apply(this, args)); return !!arr; }
    removeAllListeners(event) { const e = __perry_ee_init(this); if (event) delete e[event]; else this._events = Object.create(null); return this; }
    prependListener(event, listener) { const e = __perry_ee_init(this); (e[event] = e[event] || []).unshift(listener); return this; }
    prependOnceListener(event, listener) { __perry_ee_init(this); const wrapped = (...args) => { this.off(event, wrapped); listener(...args); }; return this.prependListener(event, wrapped); }
    listeners(event) { const e = __perry_ee_init(this); return (e[event] || []).slice(); }
    listenerCount(event) { const e = __perry_ee_init(this); return (e[event] || []).length; }
    eventNames() { const e = __perry_ee_init(this); return Object.keys(e); }
    setMaxListeners() { return this; }
    getMaxListeners() { return 10; }
}
export function once(emitter, event) {
    return new Promise((resolve) => emitter.once(event, (...args) => resolve(args)));
}
EventEmitter.EventEmitter = EventEmitter;
EventEmitter.once = once;
export const __perry_commonjs = true;
export default EventEmitter;
"#.to_string(),
        "assert" => r#"
// Stub implementation for Node.js 'assert' module
export function ok(value, message) { if (!value) throw new Error(message || 'Assertion failed'); }
export function strictEqual(a, b, message) { if (a !== b) throw new Error(message || 'Assertion failed'); }
export function deepStrictEqual(a, b, message) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message || 'Assertion failed'); }
export function notStrictEqual(a, b, message) { if (a === b) throw new Error(message || 'Assertion failed'); }
export function throws(fn, message) { try { fn(); throw new Error(message || 'Expected function to throw'); } catch (e) {} }
export function doesNotThrow(fn, message) { try { fn(); } catch (e) { throw new Error(message || 'Expected function not to throw'); } }
export function rejects(fn, message) { return fn().then(() => { throw new Error(message || 'Expected promise to reject'); }).catch(() => {}); }
export default { ok, strictEqual, deepStrictEqual, notStrictEqual, throws, doesNotThrow, rejects };
"#.to_string(),
        "url" => r#"
// Stub implementation for Node.js 'url' module
export const URL = globalThis.URL;
export const URLSearchParams = globalThis.URLSearchParams;
export function parse(urlString) { const u = new URL(urlString, 'http://localhost'); return { protocol: u.protocol, host: u.host, hostname: u.hostname, port: u.port, pathname: u.pathname, search: u.search, hash: u.hash, href: u.href }; }
export function format(urlObj) { return urlObj.href || ''; }
export function resolve(from, to) { return new URL(to, from).href; }
export default { URL, URLSearchParams, parse, format, resolve };
"#.to_string(),
        "querystring" => r#"
// Stub implementation for Node.js 'querystring' module
export function stringify(obj) { return new URLSearchParams(obj).toString(); }
export function parse(str) { const params = new URLSearchParams(str); const obj = {}; for (const [k, v] of params) obj[k] = v; return obj; }
export function escape(str) { return encodeURIComponent(str); }
export function unescape(str) { return decodeURIComponent(str); }
export default { stringify, parse, escape, unescape };
"#.to_string(),
        "tty" => r#"
// Stub implementation for Node.js 'tty' module
export function isatty() { return false; }
export class ReadStream {}
export class WriteStream {}
export default { isatty, ReadStream, WriteStream };
"#.to_string(),
        "string_decoder" => r#"
// Stub implementation for Node.js 'string_decoder' module
export class StringDecoder {
    constructor(encoding) { this.encoding = encoding || 'utf8'; }
    write(buffer) { return new TextDecoder(this.encoding).decode(buffer); }
    end(buffer) { return buffer ? this.write(buffer) : ''; }
}
export default { StringDecoder };
"#.to_string(),
        "zlib" => r#"
// Stub implementation for Node.js 'zlib' module
export function gzip() { throw new Error('zlib.gzip not supported'); }
export function gunzip() { throw new Error('zlib.gunzip not supported'); }
export function gzipSync() { throw new Error('zlib.gzipSync not supported'); }
export function gunzipSync(data) { throw new Error('zlib.gunzipSync not supported'); }
export function deflate() { throw new Error('zlib.deflate not supported'); }
export function inflate() { throw new Error('zlib.inflate not supported'); }
export function deflateSync() { throw new Error('zlib.deflateSync not supported'); }
export function inflateSync() { throw new Error('zlib.inflateSync not supported'); }
export function brotliCompress() { throw new Error('zlib.brotliCompress not supported'); }
export function brotliDecompress() { throw new Error('zlib.brotliDecompress not supported'); }
export function brotliCompressSync() { throw new Error('zlib.brotliCompressSync not supported'); }
export function brotliDecompressSync() { throw new Error('zlib.brotliDecompressSync not supported'); }
export function createGzip() { throw new Error('zlib.createGzip not supported'); }
export function createGunzip() { throw new Error('zlib.createGunzip not supported'); }
export function createDeflate() { throw new Error('zlib.createDeflate not supported'); }
export function createInflate() { throw new Error('zlib.createInflate not supported'); }
export default { gzip, gunzip, gzipSync, gunzipSync, deflate, inflate, deflateSync, inflateSync, brotliCompress, brotliDecompress, brotliCompressSync, brotliDecompressSync, createGzip, createGunzip, createDeflate, createInflate };
"#.to_string(),
        "async_hooks" => r#"
// Lightweight implementation for Node.js 'async_hooks' module.
// This is intentionally self-contained because built-in modules are loaded as
// synthetic ESM sources by perry-jsruntime. It models the public lifecycle
// enough for tracers that use createHook(), AsyncResource, and async ids.
let __perryNextAsyncId = 1;
let __perryExecutionAsyncId = 0;
let __perryTriggerAsyncId = 0;
let __perryInHookCallback = false;
const __perryExecutionStack = [];
const __perryHooks = [];

function __perryEnabledHooks() {
    return __perryHooks.filter((hook) => hook && hook.enabled);
}

function __perryEmit(name, ...args) {
    if (__perryInHookCallback) return;
    const enabled = __perryEnabledHooks();
    if (enabled.length === 0) return;
    __perryInHookCallback = true;
    try {
        for (const hook of enabled) {
            const cb = hook.callbacks && hook.callbacks[name];
            if (typeof cb === "function") cb(...args);
        }
    } finally {
        __perryInHookCallback = false;
    }
}

function __perryEnter(asyncId, triggerAsyncId) {
    __perryExecutionStack.push([__perryExecutionAsyncId, __perryTriggerAsyncId]);
    __perryExecutionAsyncId = asyncId;
    __perryTriggerAsyncId = triggerAsyncId;
    __perryEmit("before", asyncId);
}

function __perryLeave(asyncId) {
    try {
        __perryEmit("after", asyncId);
    } finally {
        const previous = __perryExecutionStack.pop() || [0, 0];
        __perryExecutionAsyncId = previous[0];
        __perryTriggerAsyncId = previous[1];
    }
}

function __perryAllocateResource(type, resource, triggerAsyncId = __perryExecutionAsyncId) {
    const asyncId = __perryNextAsyncId++;
    __perryEmit("init", asyncId, String(type || "AsyncResource"), triggerAsyncId, resource);
    return { asyncId, triggerAsyncId, destroyed: false };
}

function __perryDestroy(state) {
    if (!state || state.destroyed) return;
    state.destroyed = true;
    __perryEmit("destroy", state.asyncId);
}

function __perryWrapCallback(type, callback) {
    if (typeof callback !== "function") return callback;
    const state = __perryAllocateResource(type, callback);
    return function (...args) {
        __perryEnter(state.asyncId, state.triggerAsyncId);
        try {
            return callback.apply(this, args);
        } finally {
            __perryLeave(state.asyncId);
            __perryDestroy(state);
        }
    };
}

export class AsyncResource {
    constructor(type, options = {}) {
        const triggerAsyncId = options && Object.prototype.hasOwnProperty.call(options, "triggerAsyncId")
            ? Number(options.triggerAsyncId)
            : __perryExecutionAsyncId;
        this.__perryAsyncState = __perryAllocateResource(type || "AsyncResource", this, triggerAsyncId);
    }
    runInAsyncScope(fn, thisArg, ...args) {
        const state = this.__perryAsyncState;
        __perryEnter(state.asyncId, state.triggerAsyncId);
        try { return fn.apply(thisArg, args); }
        finally { __perryLeave(state.asyncId); }
    }
    emitDestroy() { __perryDestroy(this.__perryAsyncState); return this; }
    asyncId() { return this.__perryAsyncState.asyncId; }
    triggerAsyncId() { return this.__perryAsyncState.triggerAsyncId; }
    bind(fn) {
        const ar = this;
        return function (...args) { return ar.runInAsyncScope(fn, this, ...args); };
    }
    static bind(fn, type, thisArg) {
        const ar = new AsyncResource(type || "bound-anonymous-fn");
        return ar.bind(thisArg !== undefined ? fn.bind(thisArg) : fn);
    }
}

export class AsyncLocalStorage {
    constructor() { this._store = undefined; }
    run(store, fn, ...args) {
        const prev = this._store;
        this._store = store;
        try { return fn(...args); } finally { this._store = prev; }
    }
    exit(fn, ...args) {
        const prev = this._store;
        this._store = undefined;
        try { return fn(...args); } finally { this._store = prev; }
    }
    getStore() { return this._store; }
    enterWith(store) { this._store = store; }
    disable() { this._store = undefined; }
}

export function executionAsyncId() { return __perryExecutionAsyncId; }
export function executionAsyncResource() { return {}; }
export function triggerAsyncId() { return __perryTriggerAsyncId; }
export function createHook(callbacks = {}) {
    const hook = {
        callbacks,
        enabled: false,
        enable() {
            if (!__perryHooks.includes(hook)) __perryHooks.push(hook);
            hook.enabled = true;
            return hook;
        },
        disable() { hook.enabled = false; return hook; },
    };
    return hook;
}

const __perryNativeSetTimeout = globalThis.setTimeout;
if (typeof __perryNativeSetTimeout === "function" && !__perryNativeSetTimeout.__perryAsyncHooksWrapped) {
    const wrapped = function (callback, delay, ...args) {
        return __perryNativeSetTimeout.call(this, __perryWrapCallback("Timeout", callback), delay, ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.setTimeout = wrapped;
}

const __perryNativeSetImmediate = globalThis.setImmediate;
if (typeof __perryNativeSetImmediate === "function" && !__perryNativeSetImmediate.__perryAsyncHooksWrapped) {
    const wrapped = function (callback, ...args) {
        return __perryNativeSetImmediate.call(this, __perryWrapCallback("Immediate", callback), ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.setImmediate = wrapped;
}

if (globalThis.process && typeof globalThis.process.nextTick === "function" && !globalThis.process.nextTick.__perryAsyncHooksWrapped) {
    const nativeNextTick = globalThis.process.nextTick;
    const wrapped = function (callback, ...args) {
        return nativeNextTick.call(this, __perryWrapCallback("TickObject", callback), ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.process.nextTick = wrapped;
}

const __perryNativePromise = globalThis.Promise;
if (typeof __perryNativePromise === "function" && !__perryNativePromise.__perryAsyncHooksWrapped) {
    class PerryAsyncHookPromise extends __perryNativePromise {
        constructor(executor) {
            let state;
            super((resolve, reject) => {
                state = __perryAllocateResource("PROMISE", undefined);
                const settle = (fn) => (value) => {
                    if (!state.destroyed) {
                        __perryEmit("promiseResolve", state.asyncId);
                        __perryDestroy(state);
                    }
                    return fn(value);
                };
                return executor(settle(resolve), settle(reject));
            });
            this.__perryAsyncState = state;
        }
        static get [Symbol.species]() { return __perryNativePromise; }
    }
    PerryAsyncHookPromise.__perryAsyncHooksWrapped = true;
    globalThis.Promise = PerryAsyncHookPromise;
}

export default { AsyncResource, AsyncLocalStorage, executionAsyncId, executionAsyncResource, triggerAsyncId, createHook };
"#.to_string(),
        // Issue #755: Node built-in subpath aliases. These ship in real Node
        // as separate module IDs (`fs/promises`, `stream/promises`, etc.)
        // and packages like colyseus import them directly. Stubs mirror the
        // promise-flavored shape of the corresponding base module.
        "fs/promises" => r#"
// Stub implementation for Node.js 'fs/promises' module
export async function readFile() { throw new Error('fs.promises.readFile not supported'); }
export async function writeFile() { throw new Error('fs.promises.writeFile not supported'); }
export async function appendFile() { throw new Error('fs.promises.appendFile not supported'); }
export async function access() { throw new Error('fs.promises.access not supported'); }
export async function stat() { throw new Error('fs.promises.stat not supported'); }
export async function lstat() { throw new Error('fs.promises.lstat not supported'); }
export async function mkdir() { throw new Error('fs.promises.mkdir not supported'); }
export async function readdir() { return []; }
export async function rmdir() { throw new Error('fs.promises.rmdir not supported'); }
export async function rm() { throw new Error('fs.promises.rm not supported'); }
export async function unlink() { throw new Error('fs.promises.unlink not supported'); }
export async function rename() { throw new Error('fs.promises.rename not supported'); }
export async function copyFile() { throw new Error('fs.promises.copyFile not supported'); }
export async function chmod() { throw new Error('fs.promises.chmod not supported'); }
export async function chown() { throw new Error('fs.promises.chown not supported'); }
export async function realpath() { throw new Error('fs.promises.realpath not supported'); }
export async function symlink() { throw new Error('fs.promises.symlink not supported'); }
export async function readlink() { throw new Error('fs.promises.readlink not supported'); }
export async function open() { throw new Error('fs.promises.open not supported'); }
export async function utimes() { throw new Error('fs.promises.utimes not supported'); }
export async function truncate() { throw new Error('fs.promises.truncate not supported'); }
export async function cp() { throw new Error('fs.promises.cp not supported'); }
export const constants = {};
export default { readFile, writeFile, appendFile, access, stat, lstat, mkdir, readdir, rmdir, rm, unlink, rename, copyFile, chmod, chown, realpath, symlink, readlink, open, utimes, truncate, cp, constants };
"#.to_string(),
        "stream/promises" => r#"
// Stub implementation for Node.js 'stream/promises' module
export async function pipeline() { throw new Error('stream.promises.pipeline not supported'); }
export async function finished() { throw new Error('stream.promises.finished not supported'); }
export default { pipeline, finished };
"#.to_string(),
        "stream/consumers" => r#"
// Stub implementation for Node.js 'stream/consumers' module
export async function arrayBuffer() { throw new Error('stream.consumers.arrayBuffer not supported'); }
export async function blob() { throw new Error('stream.consumers.blob not supported'); }
export async function buffer() { throw new Error('stream.consumers.buffer not supported'); }
export async function json() { throw new Error('stream.consumers.json not supported'); }
export async function text() { throw new Error('stream.consumers.text not supported'); }
export default { arrayBuffer, blob, buffer, json, text };
"#.to_string(),
        "stream/web" => r#"
// Stub implementation for Node.js 'stream/web' module
export const ReadableStream = globalThis.ReadableStream;
export const WritableStream = globalThis.WritableStream;
export const TransformStream = globalThis.TransformStream;
export const ByteLengthQueuingStrategy = globalThis.ByteLengthQueuingStrategy;
export const CountQueuingStrategy = globalThis.CountQueuingStrategy;
export default { ReadableStream, WritableStream, TransformStream, ByteLengthQueuingStrategy, CountQueuingStrategy };
"#.to_string(),
        "dns/promises" => r#"
// Stub implementation for Node.js 'dns/promises' module
export async function lookup() { throw new Error('dns.promises.lookup not supported'); }
export async function resolve() { throw new Error('dns.promises.resolve not supported'); }
export async function resolve4() { throw new Error('dns.promises.resolve4 not supported'); }
export async function resolve6() { throw new Error('dns.promises.resolve6 not supported'); }
export async function reverse() { throw new Error('dns.promises.reverse not supported'); }
export default { lookup, resolve, resolve4, resolve6, reverse };
"#.to_string(),
        "timers/promises" => r#"
// Stub implementation for Node.js 'timers/promises' module
export function setTimeout(ms, value) { return new Promise((resolve) => globalThis.setTimeout(() => resolve(value), ms)); }
export function setImmediate(value) { return new Promise((resolve) => globalThis.setTimeout(() => resolve(value), 0)); }
export async function* setInterval(ms, value) { while (true) { await new Promise((r) => globalThis.setTimeout(r, ms)); yield value; } }
export default { setTimeout, setImmediate, setInterval };
"#.to_string(),
        "readline/promises" => r#"
// Stub implementation for Node.js 'readline/promises' module
export class Interface {
    constructor() {}
    async question() { throw new Error('readline.promises.question not supported'); }
    close() {}
    on() { return this; }
}
export function createInterface() { return new Interface(); }
export default { Interface, createInterface };
"#.to_string(),
        "util/types" => r#"
// Stub implementation for Node.js 'util/types' module
export function isDate(v) { return v instanceof Date; }
export function isRegExp(v) { return v instanceof RegExp; }
export function isMap(v) { return v instanceof Map; }
export function isSet(v) { return v instanceof Set; }
export function isPromise(v) { return v && typeof v.then === 'function'; }
export function isArrayBuffer(v) { return v instanceof ArrayBuffer; }
export function isTypedArray(v) { return ArrayBuffer.isView(v) && !(v instanceof DataView); }
export function isUint8Array(v) { return v instanceof Uint8Array; }
export default { isDate, isRegExp, isMap, isSet, isPromise, isArrayBuffer, isTypedArray, isUint8Array };
"#.to_string(),
        "assert/strict" => r#"
// Stub implementation for Node.js 'assert/strict' module
export function ok(value, message) { if (!value) throw new Error(message || 'Assertion failed'); }
export function strictEqual(a, b, message) { if (a !== b) throw new Error(message || 'Assertion failed'); }
export function deepStrictEqual(a, b, message) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message || 'Assertion failed'); }
export function notStrictEqual(a, b, message) { if (a === b) throw new Error(message || 'Assertion failed'); }
export default { ok, strictEqual, deepStrictEqual, notStrictEqual };
"#.to_string(),
        "perf_hooks" => r#"
// Stub implementation for Node.js 'perf_hooks' module. NestJS
// (`@nestjs/core/injector/module-token-factory.js`) reaches into
// `performance.now()` during dynamic-module compile; without this stub
// the import resolves to `{}` and `performance.now` throws. Minimum
// viable surface — just enough to keep NestJS's serialization timer
// running. (#1021.)
export const performance = {
    now: () => (typeof Date !== 'undefined' ? Date.now() : 0),
    timeOrigin: 0,
    mark() {},
    measure() {},
    clearMarks() {},
    clearMeasures() {},
    getEntries: () => [],
    getEntriesByName: () => [],
    getEntriesByType: () => [],
    nodeTiming: {},
    timerify: (fn) => fn,
    eventLoopUtilization: () => ({ idle: 0, active: 0, utilization: 0 }),
};
export class PerformanceObserver {
    constructor() {}
    observe() {}
    disconnect() {}
    takeRecords() { return []; }
}
PerformanceObserver.supportedEntryTypes = [];
export const constants = {};
export const monitorEventLoopDelay = () => ({ enable() {}, disable() {}, reset() {} });
export default { performance, PerformanceObserver, constants, monitorEventLoopDelay };
"#.to_string(),
        _ => format!(r#"
// Empty stub for unsupported Node.js built-in: {}
export default {{}};
"#, name),
    }
}
