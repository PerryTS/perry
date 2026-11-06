Whole-program facts tests: a private method called from the constructor of a
class that a mixin extends now has an activation-path fixture. Its body runs
through the call and the call's `this` binding; leaving out both excludes it,
and without private-call routing the fixture fails. Test only, no behavior
change.
