const p:any={...{},key:"p",use(){return()=>this.key}}; function F(){}; Object.assign(F,p); class T extends F{static key="class"} console.log(T.use()());
