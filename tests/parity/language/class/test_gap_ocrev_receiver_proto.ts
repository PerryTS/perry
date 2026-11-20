const p:any={...{},key:"p",use(){return()=>this.key}}; class T{}; Object.setPrototypeOf(T,p); (T as any).key="class"; console.log((T as any).use()());
