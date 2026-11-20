import domain from "node:domain";

try {
  (domain.Domain as any)();
  console.log("returned");
} catch (error: any) {
  console.log(error.name);
}
