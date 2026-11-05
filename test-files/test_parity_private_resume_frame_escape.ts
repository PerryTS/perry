function* values() { yield 1; yield 2; return 3; }
const it = values();
const alias = it;
console.log(it === alias, alias.next().value, it.next().value, alias.return(9).value, it.next().done);
