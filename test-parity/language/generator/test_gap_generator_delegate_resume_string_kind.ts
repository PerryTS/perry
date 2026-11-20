const marker = { value: 7, done: false, extra: "kept" };
const custom = {
 i:0, [Symbol.iterator]() { return this; },
 next(v:unknown) { this.i++; if(this.i===1)return marker; if(this.i===2)return {value:8,done:false}; return {value:"inner-ret",done:true}; },
 return(v:unknown) { return {value:v,done:true}; }
};
function* outer(){ const r=yield* custom; console.log(r);return "outer-end"; }
const o=outer();console.log(JSON.stringify(o.next()));console.log(JSON.stringify(o.next("s2")));console.log(JSON.stringify(o.next("s3")));
custom.i=0;const o2=outer();o2.next();console.log(JSON.stringify(o2.return("early")));
