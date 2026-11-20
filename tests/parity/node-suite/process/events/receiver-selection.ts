// Each source of the process object must keep the event hooks reachable.
function exercise(p: any, label: string) {
    const event = "nmi-receiver-selection";
    const listener = () => console.log(label);
    p.on(event, listener);
    console.log(p.emit(event));
    p.removeListener(event, listener);
    console.log(p.emit(event));
}
exercise(process, "argument");
const alias = process;
exercise(alias, "alias");
exercise(globalThis.process, "global");
exercise(globalThis["process"], "computed global");
const callback = () => exercise(process, "closure");
callback();
class Subscriber {
    run(p: any = process) { exercise(p, "default"); }
}
new Subscriber().run();
const method = "on";
const listener = () => console.log("computed method");
alias[method]("nmi-computed", listener);
alias.emit("nmi-computed");
alias.removeListener("nmi-computed", listener);
