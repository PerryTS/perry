// Test private class fields (#)

class Counter {
    #count: number;

    constructor() {
        this.#count = 0;
    }

    increment(): void {
        this.#count = this.#count + 1;
    }

    decrement(): void {
        this.#count = this.#count - 1;
    }

    getCount(): number {
        return this.#count;
    }

    setCount(value: number): void {
        this.#count = value;
    }
}

// Test basic private field access
let c = new Counter();
console.log(c.getCount());  // Should print 0

c.increment();
console.log(c.getCount());  // Should print 1

c.increment();
c.increment();
console.log(c.getCount());  // Should print 3

c.decrement();
console.log(c.getCount());  // Should print 2

c.setCount(10);
console.log(c.getCount());  // Should print 10

// Test another class with multiple private fields
class BankAccount {
    #balance: number;
    #accountNumber: number;

    constructor(accountNumber: number, initialBalance: number) {
        this.#accountNumber = accountNumber;
        this.#balance = initialBalance;
    }

    deposit(amount: number): void {
        this.#balance = this.#balance + amount;
    }

    withdraw(amount: number): number {
        if (amount <= this.#balance) {
            this.#balance = this.#balance - amount;
            return amount;
        }
        return 0;
    }

    getBalance(): number {
        return this.#balance;
    }

    getAccountNumber(): number {
        return this.#accountNumber;
    }
}

let account = new BankAccount(12345, 100);
console.log(account.getAccountNumber());  // Should print 12345
console.log(account.getBalance());        // Should print 100

account.deposit(50);
console.log(account.getBalance());        // Should print 150

let withdrawn = account.withdraw(30);
console.log(withdrawn);                   // Should print 30
console.log(account.getBalance());        // Should print 120
