// Sole function exports must use the same CJS value ABI as exports with properties.
import renamed from "./_helpers/globalget/sole_named.js";
import anonymous from "./_helpers/globalget/sole_anonymous.js";
import control from "./_helpers/globalget/with_property.js";
console.log(typeof renamed, renamed());
console.log(typeof anonymous, anonymous());
console.log(typeof control, control(), control.marker);
