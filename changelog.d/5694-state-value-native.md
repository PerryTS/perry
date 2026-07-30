Fixed native `State.value` reads returning `undefined`, which made TextField and
SecureField callbacks appear to receive empty values even though their native
change handlers delivered the correct string.
