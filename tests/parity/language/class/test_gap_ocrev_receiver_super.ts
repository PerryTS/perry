const b:any={...{},k:"b",m(){return()=>this.k}}; function F(){}; F.prototype=b; class C extends (F as any){k="i"; m(){return super.m()}} console.log(new C().m()());
