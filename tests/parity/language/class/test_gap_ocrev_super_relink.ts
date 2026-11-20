class P{static m(){return "P"}} class Other{static m(){return "Other"}} class C extends P{static m(){return super.m()}} console.log(C.m()); Object.setPrototypeOf(C,Other); console.log(C.m());
