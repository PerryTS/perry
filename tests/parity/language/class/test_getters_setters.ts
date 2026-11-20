// Test class getters and setters

class Rectangle {
    private _width: number;
    private _height: number;

    constructor(w: number, h: number) {
        this._width = w;
        this._height = h;
    }

    // Getter for computed property
    get area(): number {
        return this._width * this._height;
    }

    // Getter and setter for width
    get width(): number {
        return this._width;
    }

    set width(value: number) {
        this._width = value;
    }

    // Getter and setter for height
    get height(): number {
        return this._height;
    }

    set height(value: number) {
        this._height = value;
    }
}

// Create a rectangle
let r = new Rectangle(10, 5);

// Test getter (computed property)
console.log(r.area);  // Should print 50

// Test setter
r.width = 20;
console.log(r.area);  // Should print 100

// Test another setter
r.height = 10;
console.log(r.area);  // Should print 200

// Test direct getter access
console.log(r.width);   // Should print 20
console.log(r.height);  // Should print 10
