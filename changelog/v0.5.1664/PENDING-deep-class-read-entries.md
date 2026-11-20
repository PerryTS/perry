A property read through a deep class hierarchy is answered by the read
site's class entry instead of walking the prototype chain by name on every
read. Class entries described at most four objects, so a method or field
reached through more prototypes than that (babel's parser reads `this.match`,
`this.eat`, `this.next` through 8 to 11 layers of subclasses and plugin
mixins) refused the entry, and every read took the generic by-name walk: one
full property lookup per prototype. A class entry now describes up to 16
objects, and every hop is still compared by ShapeId on each read. A site
whose own holder entry latched on one receiver it could not describe also
keeps priming class entries for its class receivers. Formatting with
prettier: 24% fewer instructions, prototype walks from 8.2M to 0.59M per run.
