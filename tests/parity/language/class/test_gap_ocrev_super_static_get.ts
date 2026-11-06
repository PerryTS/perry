class P{static x=4; static get g(){return this.x+1}} class C extends P{static x=7; static f(){return [super.x,super.g].join(",")}} console.log(C.f());
