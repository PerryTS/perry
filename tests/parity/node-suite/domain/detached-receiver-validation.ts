import domain from "node:domain";

const enter = domain.create().enter;
try {
  enter();
  console.log("returned");
} catch (error: any) {
  console.log(error.name);
}
