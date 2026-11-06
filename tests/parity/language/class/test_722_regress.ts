// Regression check: type-erased receiver where override never set
class Greeter {
    greet(name: string): string { return `hello ${name}`; }
}

interface IGreeter { greet(name: string): string; }

const g: IGreeter = new Greeter();
console.log(g.greet("world"));
console.log(g.greet("alice"));
console.log(g.greet("bob"));
