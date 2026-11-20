let calls=0,steps=0;
function make(mode:string='ok'):any {let n=0;return {next(){steps++;if(mode==='next'&&n===1)throw 'next-error';const k=n++;if(mode==='value'&&k===1)return {done:false,get value(){throw 'value-error';}};return {value:k,done:k===3};},return(){calls++;if(mode==='return-throw')throw 'close-error';return {};},[Symbol.iterator](){return this;}};}
function test(label:string,run:()=>any){calls=0;steps=0;try{const r=run();console.log(label,'ok',String(r),calls,steps);}catch(e:any){console.log(label,typeof e==='string'?e:e.name,calls,steps);}}
test('spread-exhaustion',()=>[...make()].length);
test('spread-next-throw',()=>[...make('next')]);
test('spread-value-throw',()=>[...make('value')]);
test('from-exhaustion',()=>Array.from(make()).length);
test('from-next-throw',()=>Array.from(make('next')));
test('from-value-throw',()=>Array.from(make('value')));
test('from-map-throw',()=>Array.from(make(),()=>{throw 'map-error';}));
test('from-map-close-throw',()=>Array.from(make('return-throw'),()=>{throw 'map-error';}));
function Locked(this:any){Object.preventExtensions(this);}
test('from-define-throw',()=>Array.from.call(Locked,make()));
