// A constructor read off `globalThis` constructs the global, even when the program
// declares a class of the same name next to it. srvx's node adapter does this: it
// keeps `const NativeHeaders = globalThis.Headers`, then declares its own
// `class Headers` that wraps a lazily built native Headers. `new NativeHeaders()`
// built the program's class (an object without `append`), which failed with
// "append is not a function" on the first request header read.
const WrappedHeaders = (() => {
  const NativeHeaders = globalThis.Headers;
  class Headers {
    #raw;
    #headers;
    constructor(raw) { this.#raw = raw; }
    get _headers() {
      if (!this.#headers) {
        const headers = new NativeHeaders();
        for (let i = 0; i < this.#raw.length; i += 2) headers.append(this.#raw[i], this.#raw[i + 1]);
        this.#headers = headers;
      }
      return this.#headers;
    }
    get(name) { return this._headers.get(name); }
    getSetCookie() { return this._headers.getSetCookie(); }
    entries() { return this._headers.entries(); }
  }
  return Headers;
})();

const w = new WrappedHeaders(['Accept', 'a', 'X-Multi', 'one', 'x-multi', 'two', 'Set-Cookie', 'a=1', 'set-cookie', 'b=2']);
console.log(w.get('accept'), '|', w.get('x-multi'), '|', w.get('set-cookie'));
console.log(JSON.stringify(w.getSetCookie()));
console.log(JSON.stringify([...w.entries()]));

// The destructured form, and a second built-in.
const makers = (() => {
  const { Headers: Native, Response: NativeResponse } = globalThis;
  class Headers {
    make() { return new Native({ 'Content-Type': 'text/plain' }); }
  }
  class Response {
    make() { return new NativeResponse('body', { status: 201, headers: { 'X-A': 'b' } }); }
  }
  return [new Headers(), new Response()];
})();
const h = makers[0].make();
console.log(typeof h.append, h.get('content-type'));
const r = makers[1].make();
console.log(r.status, r.headers.get('x-a'));

// append and set on a global Headers.
const g = new globalThis.Headers();
g.append('A', '1'); g.append('a', '2'); g.set('B', '3'); g.set('b', '4');
g.append('Set-Cookie', 'x=1'); g.append('set-cookie', 'y=2');
console.log(g.get('a'), g.get('b'), g.get('set-cookie'), JSON.stringify(g.getSetCookie()));
console.log(JSON.stringify([...g]));
