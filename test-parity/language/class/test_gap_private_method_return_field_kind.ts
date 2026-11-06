class Client {
 opts:any;
 constructor(o:any){this.opts=this.#init(o);}
 #init(o:any){return {...o,ok:true};}
}
console.log(new Client({}).opts.ok);
