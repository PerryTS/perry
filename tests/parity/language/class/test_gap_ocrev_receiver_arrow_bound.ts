class P{static key="parent"; static arrow=()=>this.key; static bound=(function(){return this.key}).bind({key:"bound"})} class C extends P{static key="child"} console.log(C.arrow(),C.bound());
