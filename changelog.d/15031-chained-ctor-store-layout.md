A function constructor that chains stores into `this` (`this.a = this.b =
null`, as React's `FiberNode` does) keeps its birth layout: the layout lists
the chain's keys in the order the constructor adds them, innermost first,
instead of outer first. Every such birth used to take an out-of-order add
into the object's overflow; a React 19 keyed-list switch (#14974) runs about
5x faster.
