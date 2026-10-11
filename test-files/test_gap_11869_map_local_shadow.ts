function localMapShadow(): number[] {
  class Map {
    get(key: number): number {
      return key + 60;
    }
    set(_key: number, value: number): number {
      return value + 100;
    }
  }
  const local = new Map();
  return [local.get(2), local.set(3, 4)];
}

const result = localMapShadow();
if (result[0] !== 62 || result[1] !== 104) {
  throw new Error(`local Map shadow: ${result}`);
}
console.log("local Map shadow passed");
