// HWS companion: messaging
// HWS hook: js_runtime_install_messaging
// HWS prefixes: perry_runtime::messaging::
const Channel=MessageChannel;
const channel=new Channel();
const guard=setTimeout(()=>{console.log("missing message");process.exit(1)},3000);
channel.port2.onmessage=event=>{clearTimeout(guard);console.log(event.data.value);channel.port1.close();channel.port2.close()};
channel.port1.postMessage({value:42});
