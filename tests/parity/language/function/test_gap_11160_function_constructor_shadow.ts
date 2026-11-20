// A class expression's inferred name must not outlive its lexical binding.
function C(this: any, value = 1) { this.k = value; }
const original = C;
{ const C: any = class { k = 2 }; console.log('const-inner', new C().k); }
console.log('const-outer', (new (C as any)()).k, C === original);
{ let C: any = class { k = 3 }; console.log('let-inner', new C().k); }
console.log('let-outer', (new (C as any)(4)).k);
{ const C: any = class Named { k = 5 }; console.log('named-inner', new C().k); }
console.log('named-outer', (new (C as any)(6)).k);
{ class C { k = 7 }; console.log('decl-inner', new C().k); }
console.log('decl-outer', (new (C as any)(8)).k);
function parameter(C: any) { return new C().k; }
console.log('parameter', parameter(class { k = 9 }));
function localClass() { class C { k = 10 }; return new C().k; }
console.log('local-class', localClass());
{ const C: any = function(this: any) { this.k = 11; }; console.log('function-inner', new C().k); }
console.log('function-outer', (new (C as any)(12)).k);
const D = class C { k = 13; static make() { return new C(); } };
console.log('class-self', D.make().k);
console.log('argument', (new (C as any)((() => { const C: any = class { k = 99 }; return 14; })())).k);
