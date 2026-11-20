class C {get #a(){return 42;} static read(o){return o.#a;} } try {C.read({});} catch(e){console.log(e instanceof TypeError);}
