class C {get #a(){return 42;} static read(o){return o.#a;} } function fn(){} try {C.read(fn);} catch(e){console.log(e instanceof TypeError);}
