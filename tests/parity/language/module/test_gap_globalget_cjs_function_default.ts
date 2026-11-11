// Sole function exports must use the same CJS value ABI as exports with properties.
import renamed from "../../../fixtures/programs/_helpers/globalget/sole_named.js";
import anonymous from "../../../fixtures/programs/_helpers/globalget/sole_anonymous.js";
import control from "../../../fixtures/programs/_helpers/globalget/with_property.js";
console.log(typeof renamed, renamed());
console.log(typeof anonymous, anonymous());
console.log(typeof control, control(), control.marker);
