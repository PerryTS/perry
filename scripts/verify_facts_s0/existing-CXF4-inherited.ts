const parent: any = { inherited: "yes" };
const child: any = Object.create(parent);
child.own = "mine";
console.log(child["inherited"]);
