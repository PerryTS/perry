class CE extends Event{} console.log(Object.getPrototypeOf(CE.prototype)===Event.prototype); console.log(new CE("x").type);
