Fixed `class Sub extends mixin(Base) {}` failing to initialize the mixin's
private fields (`Cannot write private member #x to an object whose class did
not declare it`). `@npmcli/arborist`, which builds its class from mixins this
way, now constructs.
