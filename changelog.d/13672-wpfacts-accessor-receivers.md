Whole-program facts: a summarized class or literal getter/setter now runs on the
receivers its accessor group sees. Before, its instance had no `this`, so a
getter returning a field (`get table() { return this.tab; }`) had an empty
result, and a store through that result (another module's `.set` on the
returned Map) or through a setter was lost: reads of the stored entries were
proven when they must be refused. Tests cover a getter's Map taking another
module's `.set`, a setter's Map becoming the field, and the proven controls.
