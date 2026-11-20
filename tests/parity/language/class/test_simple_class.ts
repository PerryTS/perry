// Test simple class without inheritance

class Point {
    x: number;
    y: number;

    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }
}

let p = new Point(10, 20);
console.log(p.x); // Should print 10
console.log(p.y); // Should print 20
