function pick(f: any): any { return f }
console.log(pick(Number.isInteger)(3), pick(Math.abs)(-2), pick(Array.isArray)([]))
