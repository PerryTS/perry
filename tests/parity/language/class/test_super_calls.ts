// Test super method calls in class inheritance
// Critical for static typing migration: super dispatch must find parent method at correct offset

class Shape {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  area(): number {
    return 0;
  }
  describe(): string {
    return this.name + " with area " + this.area();
  }
}

class Circle extends Shape {
  radius: number;
  constructor(radius: number) {
    super("circle");
    this.radius = radius;
  }
  area(): number {
    return Math.round(Math.PI * this.radius * this.radius);
  }
}

class Square extends Shape {
  side: number;
  constructor(side: number) {
    super("square");
    this.side = side;
  }
  area(): number {
    return this.side * this.side;
  }
}

// Basic override
const c = new Circle(5);
console.log(c.area());      // 79
console.log(c.describe());  // circle with area 79

const sq = new Square(4);
console.log(sq.area());     // 16
console.log(sq.describe()); // square with area 16

// === Super constructor with multiple args ===
class Vehicle {
  make: string;
  year: number;
  constructor(make: string, year: number) {
    this.make = make;
    this.year = year;
  }
  info(): string {
    return this.year + " " + this.make;
  }
}

class Car extends Vehicle {
  doors: number;
  constructor(make: string, year: number, doors: number) {
    super(make, year);
    this.doors = doors;
  }
  fullInfo(): string {
    return this.info() + " (" + this.doors + " doors)";
  }
}

const car = new Car("Toyota", 2024, 4);
console.log(car.info());     // 2024 Toyota
console.log(car.fullInfo()); // 2024 Toyota (4 doors)
console.log(car.make);       // Toyota
console.log(car.year);       // 2024
console.log(car.doors);      // 4

// === Method override with different logic ===
class Logger {
  prefix: string;
  constructor(prefix: string) {
    this.prefix = prefix;
  }
  format(msg: string): string {
    return "[" + this.prefix + "] " + msg;
  }
}

class TimestampLogger extends Logger {
  constructor() {
    super("TS");
  }
  format(msg: string): string {
    return "[" + this.prefix + ":0] " + msg;
  }
}

const log1 = new Logger("INFO");
const log2 = new TimestampLogger();
console.log(log1.format("test"));  // [INFO] test
console.log(log2.format("test"));  // [TS:0] test

// === Polymorphism: parent reference calling child method ===
function getArea(s: Shape): number {
  return s.area();
}
console.log(getArea(c));  // 79
console.log(getArea(sq)); // 16
