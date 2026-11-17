class C { static #f=21; static #m(){return this.#f*2;} static read(){return C.#m();} } console.log(C.read());
