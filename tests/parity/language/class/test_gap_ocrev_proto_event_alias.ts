const E:any=Event; class CE extends E{} console.log(Object.getPrototypeOf(CE.prototype)===Event.prototype); console.log(new CE("x").type);
