console.log(String(42),String({value:7}),[1,2].join(","));
const date=new Date(0);console.log(date.getTime(),date.toISOString());

// A string key on an array cannot reach Date.prototype.toTemporalInstant.
function member(values: unknown[], key: string) { return values[key]; }
console.log(member([3, 4], "length"));
var E;
(function(E) { E[E["a"] = 0] = "a"; E[E["b"] = 1] = "b"; })(E || (E = {}));
console.log(Object.values(E).join(","));
class Values {
    #items = [1, 2];
    *[Symbol.iterator]() {
        for (let i = this.#items.length - 1; i >= 0; i--) yield this.#items[i];
    }
}
console.log([...new Values()].join(","));
