// Sole function exports must use the same CJS value ABI as exports with properties.
import renamed from "../../../test-files/_helpers/globalget/sole_named.js";
import anonymous from "../../../test-files/_helpers/globalget/sole_anonymous.js";
import control from "../../../test-files/_helpers/globalget/with_property.js";
console.log(typeof renamed, renamed());
console.log(typeof anonymous, anonymous());
console.log(typeof control, control(), control.marker);
