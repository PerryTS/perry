class C {static #f=0; static set #a(v){this.#f=v*2;} static read(){C.#a=21; return C.#f;} } console.log(C.read());
