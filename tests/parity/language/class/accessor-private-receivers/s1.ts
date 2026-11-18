class C { static #f = 21; static get #a() { return this.#f * 2; } static read() { return C.#a; } }
console.log(C.read());
