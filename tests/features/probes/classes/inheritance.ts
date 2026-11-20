class Base {
    name: string;

    constructor(name: string) {
        this.name = name;
    }

    greet() {
        return `hi ${this.name}`;
    }
}

class Child extends Base {
    greet() {
        return `${super.greet()}!`;
    }
}

console.log(`class:${new Child("perry").greet()}`);
