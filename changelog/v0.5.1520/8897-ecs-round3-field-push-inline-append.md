Four more general mechanisms on the ECS command path (round 3 after #8885):
`this.field.push(v)` statements bind their receiver to a local so the push
takes the inline append instead of the runtime push (and the tiny-method
allocation kernel rule sees through that expansion); the f64 typed-argument
guard and unbox are inlined at every typed dispatch; the array iteration
helpers probe the typed-array/Buffer registries only for a non-array header.
On the upstream `codehz/ecs` "5k entities: 3 commands each + sync" row the
compiled benchmark went from 4.38 ms/op to 4.15 ms/op (−5.4%, paired runs on
an idle Mac mini; Node 26.5.1 is 1.76 ms/op).
