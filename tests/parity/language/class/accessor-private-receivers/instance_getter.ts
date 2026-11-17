class C {#f=21; get #a(){return this.#f*2;} static read(o){return o.#a;} } console.log(C.read(new C()));
