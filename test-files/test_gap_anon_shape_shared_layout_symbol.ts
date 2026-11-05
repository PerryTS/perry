// The empty closed-literal shape class is copied into every module that
// receives an inlinable method building a `{}` record; its layout
// descriptor is defined once.
import { build1 } from "./_helpers/anon_shape_shared/user1.ts";
import { build2 } from "./_helpers/anon_shape_shared/user2.ts";
import { build3 } from "./_helpers/anon_shape_shared/user3.ts";
import { Query } from "./_helpers/anon_shape_shared/shape.ts";

console.log(build1(1));
console.log(build2(2));
console.log(build3(3));
const direct = new Query().params(4, "direct");
console.log(JSON.stringify(direct), Object.keys(direct).join(","));
