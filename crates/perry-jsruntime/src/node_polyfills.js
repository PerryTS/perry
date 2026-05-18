// Node.js global polyfills for V8 runtime
// These are injected before any modules are loaded

// Buffer polyfill using TextEncoder/TextDecoder
(function() {
    if (typeof globalThis.Buffer !== 'undefined') return;

    class Buffer extends Uint8Array {
        static alloc(size, fill, encoding) {
            const buf = new Buffer(size);
            if (fill !== undefined) {
                if (typeof fill === 'number') {
                    buf.fill(fill);
                } else if (typeof fill === 'string') {
                    const encoded = new TextEncoder().encode(fill);
                    for (let i = 0; i < size; i++) {
                        buf[i] = encoded[i % encoded.length];
                    }
                }
            }
            return buf;
        }

        static allocUnsafe(size) {
            return new Buffer(size);
        }

        // safe-buffer (used by express, body-parser, etc.) detects whether
        // our Buffer is "complete enough" by checking for all four of
        // .from / .alloc / .allocUnsafe / .allocUnsafeSlow. If any are
        // missing it copies static props onto a SafeBuffer shim using a
        // for-in loop, which silently skips ES class static methods
        // because those are non-enumerable. The resulting SafeBuffer is
        // then missing isBuffer / byteLength / etc. and every
        // `Buffer.isBuffer(chunk)` in express/response.js throws
        // TypeError. Providing allocUnsafeSlow keeps safe-buffer on the
        // happy path (just re-exports our Buffer), avoiding the lossy
        // for-in copy entirely.
        static allocUnsafeSlow(size) {
            return new Buffer(size);
        }

        static from(data, encodingOrOffset, length) {
            if (typeof data === 'string') {
                const encoding = encodingOrOffset || 'utf8';
                if (encoding === 'hex') {
                    const bytes = new Uint8Array(data.length / 2);
                    for (let i = 0; i < data.length; i += 2) {
                        bytes[i / 2] = parseInt(data.substr(i, 2), 16);
                    }
                    return new Buffer(bytes.buffer);
                }
                if (encoding === 'base64') {
                    const binary = atob(data);
                    const bytes = new Uint8Array(binary.length);
                    for (let i = 0; i < binary.length; i++) {
                        bytes[i] = binary.charCodeAt(i);
                    }
                    return new Buffer(bytes.buffer);
                }
                // utf8 / utf-8 / ascii / latin1
                const encoded = new TextEncoder().encode(data);
                return new Buffer(encoded.buffer, encoded.byteOffset, encoded.byteLength);
            }
            if (data instanceof ArrayBuffer) {
                return new Buffer(data, encodingOrOffset || 0, length !== undefined ? length : data.byteLength);
            }
            if (ArrayBuffer.isView(data)) {
                return new Buffer(data.buffer, data.byteOffset, data.byteLength);
            }
            if (Array.isArray(data)) {
                return new Buffer(new Uint8Array(data).buffer);
            }
            // Buffer.from(buffer) - copy
            if (data instanceof Buffer || data instanceof Uint8Array) {
                const copy = new Uint8Array(data);
                return new Buffer(copy.buffer, copy.byteOffset, copy.byteLength);
            }
            return new Buffer(0);
        }

        static isBuffer(obj) {
            return obj instanceof Buffer;
        }

        static isEncoding(encoding) {
            return ['utf8', 'utf-8', 'ascii', 'latin1', 'binary', 'hex', 'base64', 'ucs2', 'ucs-2', 'utf16le', 'utf-16le'].includes(encoding?.toLowerCase());
        }

        static concat(list, totalLength) {
            if (totalLength === undefined) {
                totalLength = list.reduce((acc, buf) => acc + buf.length, 0);
            }
            const result = Buffer.alloc(totalLength);
            let offset = 0;
            for (const buf of list) {
                result.set(buf, offset);
                offset += buf.length;
                if (offset >= totalLength) break;
            }
            return result;
        }

        static byteLength(string, encoding) {
            if (typeof string !== 'string') return string.length;
            return new TextEncoder().encode(string).length;
        }

        static compare(a, b) {
            const len = Math.min(a.length, b.length);
            for (let i = 0; i < len; i++) {
                if (a[i] < b[i]) return -1;
                if (a[i] > b[i]) return 1;
            }
            if (a.length < b.length) return -1;
            if (a.length > b.length) return 1;
            return 0;
        }

        toString(encoding, start, end) {
            const slice = this.subarray(start || 0, end || this.length);
            encoding = encoding || 'utf8';
            if (encoding === 'hex') {
                let hex = '';
                for (let i = 0; i < slice.length; i++) {
                    hex += slice[i].toString(16).padStart(2, '0');
                }
                return hex;
            }
            if (encoding === 'base64') {
                let binary = '';
                for (let i = 0; i < slice.length; i++) {
                    binary += String.fromCharCode(slice[i]);
                }
                return btoa(binary);
            }
            // utf8 / utf-8 / ascii / latin1
            return new TextDecoder().decode(slice);
        }

        write(string, offset, length, encoding) {
            offset = offset || 0;
            const encoded = new TextEncoder().encode(string);
            const len = Math.min(encoded.length, length !== undefined ? length : this.length - offset);
            this.set(encoded.subarray(0, len), offset);
            return len;
        }

        copy(target, targetStart, sourceStart, sourceEnd) {
            targetStart = targetStart || 0;
            sourceStart = sourceStart || 0;
            sourceEnd = sourceEnd || this.length;
            const slice = this.subarray(sourceStart, sourceEnd);
            target.set(slice, targetStart);
            return slice.length;
        }

        equals(other) {
            return Buffer.compare(this, other) === 0;
        }

        compare(other) {
            return Buffer.compare(this, other);
        }

        slice(start, end) {
            const sliced = super.subarray(start, end);
            return new Buffer(sliced.buffer, sliced.byteOffset, sliced.byteLength);
        }

        subarray(start, end) {
            const sliced = super.subarray(start, end);
            return new Buffer(sliced.buffer, sliced.byteOffset, sliced.byteLength);
        }

        readUInt8(offset) { return this[offset]; }
        readUInt16BE(offset) { return (this[offset] << 8) | this[offset + 1]; }
        readUInt16LE(offset) { return this[offset] | (this[offset + 1] << 8); }
        readUInt32BE(offset) { return ((this[offset] << 24) | (this[offset + 1] << 16) | (this[offset + 2] << 8) | this[offset + 3]) >>> 0; }
        readUInt32LE(offset) { return ((this[offset + 3] << 24) | (this[offset + 2] << 16) | (this[offset + 1] << 8) | this[offset]) >>> 0; }
        readInt8(offset) { const v = this[offset]; return v > 127 ? v - 256 : v; }
        readInt16BE(offset) { const v = this.readUInt16BE(offset); return v > 32767 ? v - 65536 : v; }
        readInt16LE(offset) { const v = this.readUInt16LE(offset); return v > 32767 ? v - 65536 : v; }
        readInt32BE(offset) { return (this[offset] << 24) | (this[offset + 1] << 16) | (this[offset + 2] << 8) | this[offset + 3]; }
        readInt32LE(offset) { return (this[offset + 3] << 24) | (this[offset + 2] << 16) | (this[offset + 1] << 8) | this[offset]; }

        readBigUInt64BE(offset) {
            const hi = BigInt(this.readUInt32BE(offset));
            const lo = BigInt(this.readUInt32BE(offset + 4));
            return (hi << 32n) | lo;
        }
        readBigUInt64LE(offset) {
            const lo = BigInt(this.readUInt32LE(offset));
            const hi = BigInt(this.readUInt32LE(offset + 4));
            return (hi << 32n) | lo;
        }

        writeUInt8(value, offset) { this[offset] = value & 0xff; return offset + 1; }
        writeUInt16BE(value, offset) { this[offset] = (value >> 8) & 0xff; this[offset + 1] = value & 0xff; return offset + 2; }
        writeUInt16LE(value, offset) { this[offset] = value & 0xff; this[offset + 1] = (value >> 8) & 0xff; return offset + 2; }
        writeUInt32BE(value, offset) { this[offset] = (value >> 24) & 0xff; this[offset + 1] = (value >> 16) & 0xff; this[offset + 2] = (value >> 8) & 0xff; this[offset + 3] = value & 0xff; return offset + 4; }
        writeUInt32LE(value, offset) { this[offset] = value & 0xff; this[offset + 1] = (value >> 8) & 0xff; this[offset + 2] = (value >> 16) & 0xff; this[offset + 3] = (value >> 24) & 0xff; return offset + 4; }

        toJSON() {
            return { type: 'Buffer', data: Array.from(this) };
        }

        get offset() { return this.byteOffset; }
    }

    globalThis.Buffer = Buffer;

    // TextEncoder/TextDecoder polyfill (needed by ethers.js fetch response handling)
    if (typeof globalThis.TextEncoder === 'undefined') {
        globalThis.TextEncoder = class TextEncoder {
            encode(str) {
                const buf = [];
                for (let i = 0; i < str.length; i++) {
                    let c = str.charCodeAt(i);
                    if (c < 0x80) {
                        buf.push(c);
                    } else if (c < 0x800) {
                        buf.push(0xC0 | (c >> 6), 0x80 | (c & 0x3F));
                    } else if (c < 0xD800 || c >= 0xE000) {
                        buf.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F));
                    } else {
                        i++;
                        c = 0x10000 + (((c & 0x3FF) << 10) | (str.charCodeAt(i) & 0x3FF));
                        buf.push(0xF0 | (c >> 18), 0x80 | ((c >> 12) & 0x3F), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F));
                    }
                }
                return new Uint8Array(buf);
            }
        };
    }
    if (typeof globalThis.TextDecoder === 'undefined') {
        globalThis.TextDecoder = class TextDecoder {
            decode(buf) {
                if (!buf) return '';
                const bytes = new Uint8Array(buf.buffer || buf);
                let str = '';
                for (let i = 0; i < bytes.length; i++) {
                    str += String.fromCharCode(bytes[i]);
                }
                return str;
            }
        };
    }

    // Global object aliases (needed by ethers.js crypto-browser.js getGlobal())
    if (typeof globalThis.self === 'undefined') globalThis.self = globalThis;
    if (typeof globalThis.global === 'undefined') globalThis.global = globalThis;

    // Web Crypto API polyfill (needed by ethers.js crypto-browser.js)
    if (typeof globalThis.crypto === 'undefined') {
        globalThis.crypto = {
            getRandomValues(arr) {
                for (let i = 0; i < arr.length; i++) {
                    arr[i] = Math.floor(Math.random() * 256);
                }
                return arr;
            },
            subtle: {}
        };
    }

    // Timer globals using microtasks (no real event loop, but callbacks fire via Promise)
    let __timerId = 1;
    if (typeof globalThis.setTimeout === 'undefined') {
        globalThis.setTimeout = function(fn, delay, ...args) {
            const id = __timerId++;
            Promise.resolve().then(() => fn(...args));
            return id;
        };
        globalThis.clearTimeout = function(id) {};
        globalThis.setInterval = function(fn, delay) { return __timerId++; };
        globalThis.clearInterval = function(id) {};
    }
    // setImmediate / clearImmediate - Node-specific. express's router uses
    // setImmediate to break recursion in middleware chains. Without this
    // polyfill, every request handler throws ReferenceError and our http
    // shim returns 500. Microtask-based fallback matches the behavior of
    // setTimeout above; same caveat applies (no real event loop ordering).
    if (typeof globalThis.setImmediate === 'undefined') {
        globalThis.setImmediate = function(fn, ...args) {
            const id = __timerId++;
            Promise.resolve().then(() => { try { fn(...args); } catch (_) {} });
            return id;
        };
        globalThis.clearImmediate = function(id) {};
    }

    // fetch() polyfill using op_perry_fetch Deno op
    if (typeof globalThis.fetch === 'undefined') {
        const core = Deno.core;
        globalThis.fetch = async function(input, init) {
            const url = typeof input === 'string' ? input : input.url;
            const method = (init && init.method) || 'GET';
            let body = (init && init.body) || '';
            // Convert Uint8Array/ArrayBuffer body to string (ethers.js sends Uint8Array)
            if (body && typeof body !== 'string') {
                if (body instanceof Uint8Array || body instanceof ArrayBuffer) {
                    const bytes = body instanceof ArrayBuffer ? new Uint8Array(body) : body;
                    body = new TextDecoder().decode(bytes);
                } else {
                    body = JSON.stringify(body);
                }
            }
            const headers = {};
            if (init && init.headers) {
                if (init.headers instanceof Headers) {
                    init.headers.forEach((v, k) => { headers[k] = v; });
                } else if (typeof init.headers === 'object') {
                    Object.assign(headers, init.headers);
                }
            }
            // op_perry_fetch is async (returns a Promise). Awaiting here
            // yields back to the V8 event loop, which is required for
            // self-fetch patterns like app.listen(port, async () => {
            // await fetch("http://127.0.0.1:port/...") }) - without
            // the yield the JS-side accept loop never starts polling
            // op_perry_http_accept and the request deadlocks.
            const result = await core.ops.op_perry_fetch(url, method, body, headers);
            return {
                ok: result.status >= 200 && result.status < 300,
                status: result.status,
                statusText: result.statusText,
                headers: new Headers(result.headers),
                text: async () => result.body,
                json: async () => JSON.parse(result.body),
                arrayBuffer: async () => new TextEncoder().encode(result.body).buffer,
            };
        };
        // Headers polyfill if needed
        if (typeof globalThis.Headers === 'undefined') {
            globalThis.Headers = class Headers {
                constructor(init) {
                    this._map = {};
                    if (Array.isArray(init)) {
                        for (const [k, v] of init) {
                            this._map[k.toLowerCase()] = String(v);
                        }
                    } else if (init && typeof init === 'object') {
                        for (const [k, v] of Object.entries(init)) {
                            this._map[k.toLowerCase()] = String(v);
                        }
                    }
                }
                get(name) { return this._map[name.toLowerCase()] || null; }
                set(name, value) { this._map[name.toLowerCase()] = value; }
                has(name) { return name.toLowerCase() in this._map; }
                delete(name) { delete this._map[name.toLowerCase()]; }
                forEach(cb) { for (const [k, v] of Object.entries(this._map)) cb(v, k, this); }
                entries() { return Object.entries(this._map)[Symbol.iterator](); }
                keys() { return Object.keys(this._map)[Symbol.iterator](); }
                values() { return Object.values(this._map)[Symbol.iterator](); }
            };
        }
    }

    // AbortController polyfill
    if (typeof globalThis.AbortController === 'undefined') {
        globalThis.AbortController = class AbortController {
            constructor() {
                this.signal = { aborted: false, reason: undefined, addEventListener: () => {}, removeEventListener: () => {} };
            }
            abort(reason) {
                this.signal.aborted = true;
                this.signal.reason = reason || new Error('AbortError');
            }
        };
    }

    // URL / URLSearchParams polyfill -- needed for `@hapi/hoek` and friends
    // that reference `URL.prototype` at module-init time. Modern Node exposes these as
    // globals; deno_core does not, so without this `import "joi"` crashes with
    // `ReferenceError: URL is not defined`.
    if (typeof globalThis.URLSearchParams === 'undefined') {
        const __decodePlus = (s) => decodeURIComponent(String(s).replace(/\+/g, ' '));
        const __enc = (s) => encodeURIComponent(String(s)).replace(/%20/g, '+');
        globalThis.URLSearchParams = class URLSearchParams {
            constructor(init) {
                this._list = [];
                if (init === undefined || init === null) return;
                if (typeof init === 'string') {
                    let s = init;
                    if (s.length > 0 && s.charCodeAt(0) === 63 /* '?' */) s = s.slice(1);
                    if (s.length === 0) return;
                    for (const pair of s.split('&')) {
                        if (pair.length === 0) continue;
                        const eq = pair.indexOf('=');
                        if (eq === -1) {
                            this._list.push([__decodePlus(pair), '']);
                        } else {
                            this._list.push([__decodePlus(pair.slice(0, eq)), __decodePlus(pair.slice(eq + 1))]);
                        }
                    }
                } else if (Array.isArray(init)) {
                    for (const pair of init) {
                        if (!Array.isArray(pair) || pair.length !== 2) {
                            throw new TypeError('URLSearchParams: invalid init pair');
                        }
                        this._list.push([String(pair[0]), String(pair[1])]);
                    }
                } else if (typeof init === 'object') {
                    for (const k of Object.keys(init)) {
                        this._list.push([String(k), String(init[k])]);
                    }
                }
            }
            append(name, value) { this._list.push([String(name), String(value)]); }
            delete(name) { this._list = this._list.filter(([k]) => k !== String(name)); }
            get(name) { const e = this._list.find(([k]) => k === String(name)); return e ? e[1] : null; }
            getAll(name) { return this._list.filter(([k]) => k === String(name)).map(([, v]) => v); }
            has(name) { return this._list.some(([k]) => k === String(name)); }
            set(name, value) {
                const n = String(name);
                let found = false;
                this._list = this._list.filter(([k]) => {
                    if (k !== n) return true;
                    if (!found) { found = true; return true; }
                    return false;
                });
                if (found) {
                    for (const e of this._list) { if (e[0] === n) { e[1] = String(value); break; } }
                } else {
                    this._list.push([n, String(value)]);
                }
            }
            sort() { this._list.sort((a, b) => a[0] < b[0] ? -1 : (a[0] > b[0] ? 1 : 0)); }
            forEach(cb, thisArg) { for (const [k, v] of this._list.slice()) cb.call(thisArg, v, k, this); }
            keys() { return this._list.map(([k]) => k)[Symbol.iterator](); }
            values() { return this._list.map(([, v]) => v)[Symbol.iterator](); }
            entries() { return this._list.slice()[Symbol.iterator](); }
            [Symbol.iterator]() { return this.entries(); }
            get size() { return this._list.length; }
            toString() { return this._list.map(([k, v]) => __enc(k) + '=' + __enc(v)).join('&'); }
            get [Symbol.toStringTag]() { return 'URLSearchParams'; }
        };
    }
    if (typeof globalThis.URL === 'undefined') {
        // RFC-3986-ish parser. Good enough for joi's `URL.prototype` reference,
        // `instanceof URL`, `Object.prototype.toString.call(u) === '[object URL]'`,
        // and the common `new URL(href[, base]).{href,origin,protocol,host,hostname,port,pathname,search,hash,searchParams}` access pattern.
        const __defaultPorts = { 'http:': '80', 'https:': '443', 'ws:': '80', 'wss:': '443', 'ftp:': '21' };
        const __parseUrl = (input, base) => {
            let s = String(input).trim();
            // If base supplied and input is not absolute, resolve.
            const schemeMatch = s.match(/^([a-zA-Z][a-zA-Z0-9+.\-]*):/);
            if (!schemeMatch) {
                if (base === undefined || base === null) {
                    throw new TypeError('Invalid URL: ' + input);
                }
                const baseParsed = (base instanceof globalThis.URL) ? base : __parseUrl(base, undefined);
                const u = Object.create(URL.prototype);
                u._protocol = baseParsed._protocol;
                u._username = baseParsed._username;
                u._password = baseParsed._password;
                u._hostname = baseParsed._hostname;
                u._port = baseParsed._port;
                u._pathname = baseParsed._pathname;
                u._search = '';
                u._hash = '';
                if (s.length === 0) {
                    u._search = baseParsed._search;
                    u._hash = baseParsed._hash;
                } else if (s.charCodeAt(0) === 35 /* '#' */) {
                    u._search = baseParsed._search;
                    u._hash = s;
                } else if (s.charCodeAt(0) === 63 /* '?' */) {
                    const hashIdx = s.indexOf('#');
                    if (hashIdx === -1) { u._search = s; u._hash = ''; }
                    else { u._search = s.slice(0, hashIdx); u._hash = s.slice(hashIdx); }
                } else if (s.charCodeAt(0) === 47 /* '/' */) {
                    // path-relative
                    const { path, search, hash } = __splitPath(s);
                    u._pathname = path;
                    u._search = search;
                    u._hash = hash;
                } else {
                    // resolve against base path directory
                    const basePath = baseParsed._pathname || '/';
                    const slash = basePath.lastIndexOf('/');
                    const dir = slash === -1 ? '/' : basePath.slice(0, slash + 1);
                    const { path, search, hash } = __splitPath(dir + s);
                    u._pathname = __normalizePath(path);
                    u._search = search;
                    u._hash = hash;
                }
                return u;
            }
            const u = Object.create(URL.prototype);
            u._protocol = schemeMatch[1].toLowerCase() + ':';
            s = s.slice(schemeMatch[0].length);
            u._username = '';
            u._password = '';
            u._hostname = '';
            u._port = '';
            u._pathname = '';
            u._search = '';
            u._hash = '';
            const isSpecial = __defaultPorts.hasOwnProperty(u._protocol) || u._protocol === 'file:';
            if (s.startsWith('//')) {
                s = s.slice(2);
                // authority ends at first '/', '?', or '#'
                let authEnd = s.length;
                for (let i = 0; i < s.length; i++) {
                    const c = s.charCodeAt(i);
                    if (c === 47 || c === 63 || c === 35) { authEnd = i; break; }
                }
                const auth = s.slice(0, authEnd);
                s = s.slice(authEnd);
                const atIdx = auth.lastIndexOf('@');
                let hostPart = auth;
                if (atIdx !== -1) {
                    const userInfo = auth.slice(0, atIdx);
                    hostPart = auth.slice(atIdx + 1);
                    const colonIdx = userInfo.indexOf(':');
                    if (colonIdx === -1) {
                        u._username = userInfo;
                    } else {
                        u._username = userInfo.slice(0, colonIdx);
                        u._password = userInfo.slice(colonIdx + 1);
                    }
                }
                // IPv6 in brackets?
                if (hostPart.startsWith('[')) {
                    const closeIdx = hostPart.indexOf(']');
                    if (closeIdx === -1) throw new TypeError('Invalid URL: ' + input);
                    u._hostname = hostPart.slice(0, closeIdx + 1).toLowerCase();
                    const rest = hostPart.slice(closeIdx + 1);
                    if (rest.startsWith(':')) u._port = rest.slice(1);
                } else {
                    const colonIdx = hostPart.lastIndexOf(':');
                    if (colonIdx === -1) {
                        u._hostname = hostPart.toLowerCase();
                    } else {
                        u._hostname = hostPart.slice(0, colonIdx).toLowerCase();
                        u._port = hostPart.slice(colonIdx + 1);
                    }
                }
                // strip default port
                if (u._port !== '' && __defaultPorts[u._protocol] === u._port) u._port = '';
                if (s.length === 0) {
                    u._pathname = isSpecial ? '/' : '';
                } else {
                    const { path, search, hash } = __splitPath(s);
                    u._pathname = path === '' && isSpecial ? '/' : path;
                    u._search = search;
                    u._hash = hash;
                }
            } else {
                // No authority -- e.g. mailto:foo@bar, data:text/plain;...
                const { path, search, hash } = __splitPath(s);
                u._pathname = path;
                u._search = search;
                u._hash = hash;
            }
            return u;
        };
        const __splitPath = (s) => {
            let hash = '';
            let search = '';
            const hashIdx = s.indexOf('#');
            if (hashIdx !== -1) { hash = s.slice(hashIdx); s = s.slice(0, hashIdx); }
            const qIdx = s.indexOf('?');
            if (qIdx !== -1) { search = s.slice(qIdx); s = s.slice(0, qIdx); }
            return { path: s, search, hash };
        };
        const __normalizePath = (p) => {
            // Collapse '.' and '..' segments per WHATWG (close enough).
            if (p === '') return '';
            const leading = p.charCodeAt(0) === 47;
            const parts = p.split('/');
            const out = [];
            for (const part of parts) {
                if (part === '.') continue;
                if (part === '..') { if (out.length > 0 && out[out.length - 1] !== '') out.pop(); continue; }
                out.push(part);
            }
            let result = out.join('/');
            if (leading && !result.startsWith('/')) result = '/' + result;
            return result;
        };

        class URL {
            constructor(input, base) {
                const parsed = __parseUrl(input, base);
                this._protocol = parsed._protocol;
                this._username = parsed._username;
                this._password = parsed._password;
                this._hostname = parsed._hostname;
                this._port = parsed._port;
                this._pathname = parsed._pathname;
                this._search = parsed._search;
                this._hash = parsed._hash;
            }
            get protocol() { return this._protocol; }
            set protocol(v) { const s = String(v); this._protocol = s.endsWith(':') ? s : s + ':'; }
            get username() { return this._username; }
            set username(v) { this._username = String(v); }
            get password() { return this._password; }
            set password(v) { this._password = String(v); }
            get host() {
                if (this._hostname === '') return '';
                return this._port === '' ? this._hostname : this._hostname + ':' + this._port;
            }
            set host(v) {
                const s = String(v);
                const colon = s.lastIndexOf(':');
                if (colon === -1 || s.startsWith('[')) {
                    this._hostname = s.toLowerCase();
                    this._port = '';
                } else {
                    this._hostname = s.slice(0, colon).toLowerCase();
                    this._port = s.slice(colon + 1);
                }
            }
            get hostname() { return this._hostname; }
            set hostname(v) { this._hostname = String(v).toLowerCase(); }
            get port() { return this._port; }
            set port(v) {
                const s = String(v);
                if (s === '' || /^[0-9]+$/.test(s)) this._port = s;
            }
            get pathname() { return this._pathname; }
            set pathname(v) {
                let s = String(v);
                const hasAuthority = this._hostname !== '';
                if (hasAuthority && !s.startsWith('/')) s = '/' + s;
                this._pathname = s;
            }
            get search() { return this._search; }
            set search(v) {
                let s = String(v);
                if (s === '') { this._search = ''; return; }
                if (!s.startsWith('?')) s = '?' + s;
                this._search = s;
            }
            get searchParams() {
                if (!this._searchParams) {
                    const owner = this;
                    const sp = new globalThis.URLSearchParams(this._search);
                    // Live-sync -- every mutation rewrites _search.
                    const sync = () => { owner._search = sp._list.length === 0 ? '' : '?' + sp.toString(); };
                    const wrap = (name) => {
                        const orig = sp[name];
                        sp[name] = function (...args) { const r = orig.apply(sp, args); sync(); return r; };
                    };
                    ['append', 'delete', 'set', 'sort'].forEach(wrap);
                    this._searchParams = sp;
                }
                return this._searchParams;
            }
            get hash() { return this._hash; }
            set hash(v) {
                let s = String(v);
                if (s === '') { this._hash = ''; return; }
                if (!s.startsWith('#')) s = '#' + s;
                this._hash = s;
            }
            get origin() {
                if (this._protocol === 'http:' || this._protocol === 'https:' || this._protocol === 'ws:' || this._protocol === 'wss:' || this._protocol === 'ftp:') {
                    return this._protocol + '//' + this.host;
                }
                if (this._protocol === 'file:') return 'null';
                return 'null';
            }
            get href() {
                let out = this._protocol;
                if (this._hostname !== '' || this._protocol === 'file:') {
                    out += '//';
                    if (this._username !== '' || this._password !== '') {
                        out += this._username;
                        if (this._password !== '') out += ':' + this._password;
                        out += '@';
                    }
                    out += this._hostname;
                    if (this._port !== '') out += ':' + this._port;
                }
                out += this._pathname;
                out += this._search;
                out += this._hash;
                return out;
            }
            set href(v) {
                const parsed = __parseUrl(v, undefined);
                this._protocol = parsed._protocol;
                this._username = parsed._username;
                this._password = parsed._password;
                this._hostname = parsed._hostname;
                this._port = parsed._port;
                this._pathname = parsed._pathname;
                this._search = parsed._search;
                this._hash = parsed._hash;
                this._searchParams = undefined;
            }
            toString() { return this.href; }
            toJSON() { return this.href; }
            get [Symbol.toStringTag]() { return 'URL'; }
        }
        URL.canParse = function (input, base) {
            try { __parseUrl(input, base); return true; } catch (_e) { return false; }
        };
        URL.parse = function (input, base) {
            try { return new URL(input, base); } catch (_e) { return null; }
        };
        URL.createObjectURL = function () { return ''; };
        URL.revokeObjectURL = function () {};
        globalThis.URL = URL;
    }

    // Also provide process.env and process.version if not present
    if (typeof globalThis.process === 'undefined') {
        globalThis.process = {
            env: {},
            version: 'v20.0.0',
            versions: { node: '20.0.0' },
            platform: 'darwin',
            arch: 'arm64',
            pid: 0,
            cwd: () => '/',
            nextTick: (fn, ...args) => Promise.resolve().then(() => fn(...args)),
            stdout: { write: (s) => {} },
            stderr: { write: (s) => {} },
        };
    }
})();
