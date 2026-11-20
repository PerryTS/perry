// Class getters and setters
class Temperature {
  private _celsius: number;
  constructor(celsius: number) {
    this._celsius = celsius;
  }
  get celsius(): number { return this._celsius; }
  set celsius(val: number) { this._celsius = val; }
  get fahrenheit(): number { return this._celsius * 9 / 5 + 32; }
}

const t = new Temperature(100);
console.log(t.celsius);
console.log(t.fahrenheit);
t.celsius = 0;
console.log(t.celsius);
console.log(t.fahrenheit);
