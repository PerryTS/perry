//! Native endpoint ownership. No heap addresses, callbacks or JS values live here.
use crate::agent::{current_agent, AgentId};
use crate::thread::SerializedValue;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lease {
    pub endpoint: u64,
    generation: u64,
    agent: AgentId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Agent(AgentId),
    Transit,
}
struct Endpoint {
    peer: u64,
    generation: u64,
    owner: Owner,
    inbox: VecDeque<SerializedValue>,
    worker: Option<(AgentId, u64)>,
    dispatch_depth: usize,
    closing: bool,
}
#[derive(Default)]
struct Transport {
    next: u64,
    endpoints: HashMap<u64, Endpoint>,
}
static TRANSPORT: LazyLock<Mutex<Transport>> = LazyLock::new(|| Mutex::new(Transport::default()));

static WORKER_NOTICE: crate::feature_hooks::Hook<fn(AgentId, u64)> =
    crate::feature_hooks::Hook::empty();
pub fn register_worker_notice(notice: fn(AgentId, u64)) {
    WORKER_NOTICE.set(notice);
}
pub fn route_worker(lease: Lease, worker_id: u64) {
    let mut table = TRANSPORT.lock().unwrap();
    if let Some(endpoint) = table
        .endpoints
        .get_mut(&lease.endpoint)
        .filter(|endpoint| matches(endpoint, lease))
    {
        endpoint.worker = Some((lease.agent, worker_id));
    }
}
fn matches(endpoint: &Endpoint, lease: Lease) -> bool {
    endpoint.generation == lease.generation && endpoint.owner == Owner::Agent(lease.agent)
}
fn open(endpoint: &Endpoint, lease: Lease) -> bool {
    matches(endpoint, lease) && !endpoint.closing
}
pub fn pair() -> (Lease, Lease) {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| crate::agent::register_retire_hook(retire));
    let agent = current_agent();
    let mut table = TRANSPORT.lock().unwrap();
    let a = table.next + 1;
    let b = a + 1;
    table.next = b;
    for (id, peer) in [(a, b), (b, a)] {
        table.endpoints.insert(
            id,
            Endpoint {
                peer,
                generation: 0,
                owner: Owner::Agent(agent),
                inbox: VecDeque::new(),
                worker: None,
                dispatch_depth: 0,
                closing: false,
            },
        );
    }
    (
        Lease {
            endpoint: a,
            generation: 0,
            agent,
        },
        Lease {
            endpoint: b,
            generation: 0,
            agent,
        },
    )
}
pub fn alive(lease: Lease) -> bool {
    lease.agent == current_agent()
        && TRANSPORT
            .lock()
            .unwrap()
            .endpoints
            .get(&lease.endpoint)
            .is_some_and(|endpoint| open(endpoint, lease))
}
pub fn closed(lease: Lease) -> bool {
    let table = TRANSPORT.lock().unwrap();
    table
        .endpoints
        .get(&lease.endpoint)
        .map_or(true, |endpoint| {
            endpoint.inbox.is_empty() && !table.endpoints.contains_key(&endpoint.peer)
        })
}

pub fn pending(lease: Lease) -> bool {
    TRANSPORT
        .lock()
        .unwrap()
        .endpoints
        .get(&lease.endpoint)
        .is_some_and(|endpoint| matches(endpoint, lease) && !endpoint.inbox.is_empty())
}
pub fn receive(lease: Lease) -> Option<SerializedValue> {
    if lease.agent != current_agent() {
        return None;
    }
    let mut table = TRANSPORT.lock().unwrap();
    let endpoint = table.endpoints.get_mut(&lease.endpoint)?;
    if !matches(endpoint, lease) {
        return None;
    }
    let message = endpoint.inbox.pop_front();
    let peer = endpoint.peer;
    let finished = endpoint.worker.is_some() && endpoint.inbox.is_empty();
    let removed = if finished && !table.endpoints.contains_key(&peer) {
        table.endpoints.remove(&lease.endpoint)
    } else {
        None
    };
    drop(table);
    drop(removed);
    message
}
pub fn send(lease: Lease, message: SerializedValue) -> bool {
    let mut message = Some(message);
    let mut lost = false;
    let wake = {
        let mut table = TRANSPORT.lock().unwrap();
        let peer = table
            .endpoints
            .get(&lease.endpoint)
            .filter(|endpoint| open(endpoint, lease))
            .map(|endpoint| endpoint.peer);
        let peer = peer.filter(|id| {
            lost = matches!(message.as_ref(), Some(SerializedValue::Message { data, .. })
                if data.ports.iter().any(|ticket| ticket.endpoint == *id));
            !lost
        });
        peer.and_then(|id| table.endpoints.get_mut(&id))
            .and_then(|endpoint| {
                endpoint.inbox.push_back(message.take().unwrap());
                match endpoint.owner {
                    Owner::Agent(agent) => Some((agent, endpoint.worker)),
                    Owner::Transit => None,
                }
            })
    };
    drop(message);
    if let Some((agent, worker)) = wake {
        if let (Some((owner, id)), Some(notice)) = (worker, WORKER_NOTICE.peek()) {
            notice(owner, id);
        }
        crate::event_pump::notify_agent_work(agent);
    }
    lost
}
/// Legacy IPC enters the same inbox, never a second queue authority.
pub fn enqueue(lease: Lease, message: SerializedValue) {
    let mut message = Some(message);
    {
        let mut table = TRANSPORT.lock().unwrap();
        if let Some(endpoint) = table
            .endpoints
            .get_mut(&lease.endpoint)
            .filter(|endpoint| matches(endpoint, lease))
        {
            endpoint.inbox.push_back(message.take().unwrap());
        }
    }
    drop(message);
}
pub fn close(lease: Lease) {
    let removed = {
        let mut table = TRANSPORT.lock().unwrap();
        let Some(endpoint) = table
            .endpoints
            .get_mut(&lease.endpoint)
            .filter(|endpoint| matches(endpoint, lease))
        else {
            return;
        };
        if endpoint.dispatch_depth != 0 {
            endpoint.closing = true;
            return;
        }
        remove_pair(&mut table, lease.endpoint)
    };
    dispose(removed);
}

/// A closing dispatch retains its inbox, but cannot send or transfer its lease.
pub struct DispatchGuard(Lease);

pub fn begin_dispatch(lease: Lease) -> Option<DispatchGuard> {
    if lease.agent != current_agent() {
        return None;
    }
    let mut table = TRANSPORT.lock().unwrap();
    let endpoint = table.endpoints.get_mut(&lease.endpoint)?;
    if !open(endpoint, lease) {
        return None;
    }
    endpoint.dispatch_depth += 1;
    Some(DispatchGuard(lease))
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        let removed = {
            let mut table = TRANSPORT.lock().unwrap();
            let Some(endpoint) = table
                .endpoints
                .get_mut(&self.0.endpoint)
                .filter(|endpoint| matches(endpoint, self.0))
            else {
                return;
            };
            endpoint.dispatch_depth -= 1;
            if endpoint.closing && endpoint.dispatch_depth == 0 {
                remove_pair(&mut table, self.0.endpoint)
            } else {
                Vec::new()
            }
        };
        dispose(removed);
    }
}
fn remove_pair(table: &mut Transport, id: u64) -> Vec<Endpoint> {
    let mut removed = Vec::new();
    if let Some(endpoint) = table.endpoints.remove(&id) {
        // Tickets and wrappers own endpoints independently of entanglement.
        // Only a wrapperless Worker adapter can retire when its inbox drains.
        let pending_peer = table
            .endpoints
            .get(&endpoint.peer)
            .is_some_and(|peer| peer.worker.is_none() || !peer.inbox.is_empty());
        if !pending_peer {
            if let Some(peer) = table.endpoints.remove(&endpoint.peer) {
                removed.push(peer);
            }
        }
        removed.push(endpoint);
    }
    removed
}
fn dispose(removed: Vec<Endpoint>) {
    for endpoint in &removed {
        let peer_owner = TRANSPORT
            .lock()
            .unwrap()
            .endpoints
            .get(&endpoint.peer)
            .map(|peer| peer.owner);
        if let Some(Owner::Agent(agent)) = peer_owner {
            crate::event_pump::notify_agent_work(agent);
        }
        if let Owner::Agent(agent) = endpoint.owner {
            crate::event_pump::notify_agent_work(agent);
        }
    }
    // Queues can contain tickets whose final Drop re-enters TRANSPORT.
    drop(removed);
}
fn retire(agent: AgentId) {
    let removed = {
        let mut table = TRANSPORT.lock().unwrap();
        let ids: Vec<_> = table
            .endpoints
            .iter()
            .filter_map(|(&id, endpoint)| (endpoint.owner == Owner::Agent(agent)).then_some(id))
            .collect();
        ids.into_iter()
            .flat_map(|id| remove_pair(&mut table, id))
            .collect()
    };
    dispose(removed);
}

#[derive(Debug)]
pub struct Ticket {
    endpoint: u64,
    generation: u64,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        let removed = {
            let mut table = TRANSPORT.lock().unwrap();
            if table.endpoints.get(&self.endpoint).is_some_and(|endpoint| {
                endpoint.owner == Owner::Transit && endpoint.generation == self.generation
            }) {
                remove_pair(&mut table, self.endpoint)
            } else {
                Vec::new()
            }
        };
        dispose(removed);
    }
}
pub fn commit(leases: &[Lease], source: Option<Lease>) -> Result<Vec<Arc<Ticket>>, String> {
    let mut table = TRANSPORT.lock().unwrap();
    if source.is_some_and(|lease| {
        !table
            .endpoints
            .get(&lease.endpoint)
            .is_some_and(|endpoint| open(endpoint, lease))
    }) || leases.iter().any(|lease| {
        !table
            .endpoints
            .get(&lease.endpoint)
            .is_some_and(|endpoint| open(endpoint, *lease))
    }) {
        return Err("Cannot clone object of unsupported type.".into());
    }
    Ok(leases
        .iter()
        .map(|lease| {
            let endpoint = table.endpoints.get_mut(&lease.endpoint).unwrap();
            endpoint.dispatch_depth = 0;
            endpoint.generation += 1;
            endpoint.owner = Owner::Transit;
            Arc::new(Ticket {
                endpoint: lease.endpoint,
                generation: endpoint.generation,
            })
        })
        .collect())
}
pub fn adopt(ticket: &Ticket) -> Option<Lease> {
    let mut table = TRANSPORT.lock().unwrap();
    let endpoint = table.endpoints.get_mut(&ticket.endpoint)?;
    if endpoint.generation != ticket.generation || endpoint.owner != Owner::Transit {
        return None;
    }
    endpoint.owner = Owner::Agent(current_agent());
    Some(Lease {
        endpoint: ticket.endpoint,
        generation: ticket.generation,
        agent: current_agent(),
    })
}
