const custom = {
 [Symbol.iterator]() { return this; },
 next() { return {value:1,done:false}; },
 return(v:any) { return {value:v,done:true}; }
};
function* outer(){yield* custom;}
const o=outer();o.next();console.log(o.return(77).value+1);
