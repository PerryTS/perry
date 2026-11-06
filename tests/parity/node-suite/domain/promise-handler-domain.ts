import domain from "node:domain";

const producer = domain.create();
const consumer = domain.create();
let promise: Promise<number>;
producer.run(() => promise = Promise.resolve(42));
consumer.run(() =>
  promise!.then((value) => console.log(value, domain.active === consumer))
);
