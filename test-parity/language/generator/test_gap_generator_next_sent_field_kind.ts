class Resumed { value:any; constructor(v:any){this.value=v;} }
function* single(){const v=yield 1;return new Resumed(v);}
const g=single();g.next();
const record=g.next("sent").value;
console.log(record.value);
