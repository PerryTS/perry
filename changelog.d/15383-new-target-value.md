Fixed: a constructor that returns `Object.create(new.target.prototype)` hands its class's methods an object whose own properties they can read, instead of `undefined`.
