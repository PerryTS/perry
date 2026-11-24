On the web target a class's members are the properties Node defines: `C.prototype` is
read-only, instance and static getters and setters have descriptors on the prototype and
on the class (static getters read `undefined` before), and a static field redefined with
`Object.defineProperty` reads and stores as redefined (#15340).
