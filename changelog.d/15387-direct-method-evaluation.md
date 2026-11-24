Fixed: in a class expression evaluated more than once (a mixin or class factory), `super` inside a method called directly reaches the parent of that instance's own class, not of the last one created.
