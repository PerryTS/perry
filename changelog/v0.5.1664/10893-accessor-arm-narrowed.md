fix(runtime): the #10893 accessor arm only fires for a genuine accessor.

The first version of the dispatch tower's accessor arm did an ordinary by-name read and invoked whatever came back. That resurrected members the tower had deliberately refused: after `delete C.prototype.m`, `obj.m()` stopped throwing, and the imported-clone guards lost a prototype semantic (`issue_9180_decl_prototype_reverse_lookup`, `issue_8693_imported_this_specialization` both regressed).

The arm now fires only when the receiver's class chain actually declares an accessor of that name, and never for a key `delete` removed — so it still covers the case it exists for (a getter returning a callable, `c.g(1)`) and nothing else.
