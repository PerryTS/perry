console.log(JSON.stringify({v:1, toJSON(){return this.v}})) // 1
console.log(await {v:2, then(r){r(this.v)}}) // 2
console.log([...{a:[3], [Symbol.iterator](){return this.a.values()}}]) // [ 3 ]
console.log({n:4, valueOf(){return this.n}} + 1) // 5
console.log(new Proxy({}, {tag:"h", get(){return this.tag}}).x) // h
export {};
