const p:any={...{},key:"p",use(){return()=>this.key}}; (Function.prototype as any).use=p.use; class T{static key="class"} console.log((T as any).use()()); delete (Function.prototype as any).use;
