const parent = new Function('x', 'return function(y) { return x+y; };');
const child = parent(30);
for(let i=0;i<40000;i++){const a={i, text:`ast-${i}`};if(a.i!==i)throw new Error('allocation');}
if(child(7)!==37 || parent(20)(17)!==37)throw new Error('parsed function lifetime');
const fresh = new Function('return 42;');
if(fresh()!==42)throw new Error('fresh function');
const throwing = new Function('throw 7;');
for(let i=0;i<10;i++){try{throwing();}catch(e){if(e!==7)throw e;}}
console.log('dyn-eval-owners',child(7),fresh());
