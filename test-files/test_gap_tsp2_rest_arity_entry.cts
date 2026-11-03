function emit(label, value) { console.log(label, JSON.stringify(value)); }
function make() {
  return {
    fewer: function(a) { return [a, arguments.length, Array.from(arguments)]; },
    more: function(a,b,c) { return [a,b,c,arguments.length,Array.from(arguments)]; },
    rest: function(a,...xs) { return [a,xs,arguments.length,Array.from(arguments)]; },
    defaults: function(a=11,b=22,...xs) { return [a,b,xs,arguments.length]; },
    strict: function(a,b) { 'use strict'; arguments[0]=31; b=42; return [a,b,arguments[0],arguments[1],arguments.length]; },
    sloppy: function(a,b) { arguments[0]=31; b=42; return [a,b,arguments[0],arguments[1],arguments.length]; },
    gen: function*(a,...xs) { yield [a,xs,arguments.length]; return xs.length; },
    async: async function(a,...xs) { await 0; return [a,xs,arguments.length]; }
  };
}
const f=make();
emit('fewer',f.fewer(1,2,3)); emit('more',f.more(1));
emit('rest',f.rest(1,2,3)); emit('rest-empty',f.rest());
emit('default-missing',f.defaults()); emit('default-explicit',f.defaults(undefined,4,5,6));
emit('lengths',[f.fewer.length,f.more.length,f.rest.length,f.defaults.length,f.gen.length,f.async.length]);
emit('strict-over',f.strict(1,2,3)); emit('strict-under',f.strict(1));
emit('sloppy-over',f.sloppy(1,2,3)); emit('sloppy-under',f.sloppy(1));
const g=f.gen(1,2,3); emit('gen-1',g.next()); emit('gen-2',g.next());
const g0=f.gen(); emit('gen-empty',g0.next());
f.async(1,2,3).then(x=>emit('async',x)); f.async().then(x=>emit('async-empty',x));
const apply={run:function() { return f.more.apply(this,arguments); }}; emit('apply',apply.run(7,8,9,10));
emit('spread',f.more(...[7]));
const bound=f.rest.bind(null,4); emit('bound',bound(5,6));emit('bound-length',bound.length);
// Cross the old 16-argument dispatch band in both directions.
const wide = {run: function(a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t) {
  return [a,t,arguments.length];
}};
emit("wide-short",wide.run(7));
emit("wide-extra",wide.run(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21));
const wideRest = {run: function(a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,...tail) {
  return [a,r,tail];
}};
emit("wide-rest-short",wideRest.run(7));
emit("wide-rest-extra",wideRest.run(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21));
// Pointer-bearing values must survive allocations in the entry and body.
const retain = {run: (...xs) => { if (typeof gc === "function") gc(); return xs; }};
let retainedSum=0;
for(let i=0;i<200;i++) {
  const xs=retain.run({x:i}, "caller-owned-long-argument-"+i, {x:i+1});
  retainedSum += xs[0].x + xs[1].length + xs[2].x;
}
emit("retained",retainedSum);

class CallEntryMethods {
  run(a,...xs) { return [this.seed,a,xs,arguments.length]; }
  static run(a,b,c) { return [a,b,c,arguments.length]; }
  constructor() { this.seed=12; }
}
const methodOwner=new CallEntryMethods();
const restMethod=methodOwner.run.bind(methodOwner);
emit("class-rest",restMethod(1,2,3));
const staticMethod=CallEntryMethods.run.bind(null);
emit("class-short",staticMethod(7));

// Exceptions must unwind through the adapters, including stack arguments.
const throwingRest = {run: function(a,...xs) { throw a + xs.length; }};
try { throwingRest.run.apply(null,[7,8,9]); }
catch(value) { emit("throw-rest-entry",value); }
const throwingWideRest = {run: function(a,b,c,d,e,f,g,h,i,j,...xs) { throw j + xs.length; }};
try { throwingWideRest.run.apply(null,[1,2,3,4,5,6,7,8,9,10,11]); }
catch(value) { emit("throw-wide-rest-entry",value); }
const throwingWideMissing = {run: function(a,b,c,d,e,f,g,h,i,j) { throw j === undefined ? 84 : 85; }};
try { throwingWideMissing.run.apply(null,[1]); }
catch(value) { emit("throw-wide-missing-entry",value); }
