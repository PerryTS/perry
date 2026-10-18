import { make, First } from './fixtures/awb2-static-proto/exported-factory.ts';
const Second = make();
console.log('exported-factory', First !== Second, First.prototype !== Second.prototype);
First.prototype.own = () => 37;
console.log('isolated-export', new First().run(), new Second().run());
