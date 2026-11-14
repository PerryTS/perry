On the web target a class used as a value is a constructor: a class held in a
variable or an object (`const KK = K; new KK()`, `new r.C()`) constructs its
instances, and `instanceof K` holds, instead of the value being `undefined`.
