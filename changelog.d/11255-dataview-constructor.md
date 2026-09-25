Fix DataView subclass instances reporting the global DataView constructor instead of inheriting the subclass constructor. Constructor reads now follow the actual prototype chain, preserving own-property precedence, inherited overrides, null prototypes, and getter receiver identity. Changes to DataView.prototype.constructor are honored as well.

The receiver and key remain rooted while the intrinsic prototype is materialized. Regression tests cover runtime property lookup and native output against Node 26.5.1.
