const Port = (globalThis as any).MessagePort;
const Channel = (globalThis as any).MessageChannel;
const Broadcast = (globalThis as any).BroadcastChannel;

function reject(label: string, action: () => unknown) {
  try {
    action();
    console.log(label, "created");
  } catch (error: any) {
    console.log(label, error.name, error.code ?? "", error.message);
  }
}

reject("port call:", () => Port());
reject("port new:", () => new Port());
reject("port apply:", () => Reflect.apply(Port, undefined, []));
reject("port reflect:", () => Reflect.construct(Port, []));
reject("channel call:", () => Channel());
reject("channel apply:", () => Reflect.apply(Channel, undefined, []));
reject("broadcast call:", () => Broadcast("policy"));
reject("broadcast apply:", () => Reflect.apply(Broadcast, undefined, ["policy"]));

const channel = Reflect.construct(Channel, []);
console.log("valid ports:", channel.port1 instanceof Port, channel.port2.constructor === Port);
channel.port1.close();
channel.port2.close();
const broadcast = new Broadcast("policy");
console.log("valid broadcast:", broadcast.name);
broadcast.close();
