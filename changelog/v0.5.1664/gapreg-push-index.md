`push` on a frozen array reported a garbage index in its TypeError
("Cannot add property 2571834624, ...") when the call went through dynamic
dispatch. The message now reads the array's current length from its resolved
storage, matching Node ("Cannot add property 2, object is not extensible").
