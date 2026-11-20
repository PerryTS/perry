import domain from "node:domain";

for (const name of ["Domain", "createDomain", "create", "active", "_stack"]) {
  const descriptor = Object.getOwnPropertyDescriptor(domain, name)!;
  console.log(
    name,
    descriptor.enumerable,
    descriptor.configurable,
    descriptor.writable,
  );
}
