//! Agent-loop operations installed for programs that can wait for native IO.
use super::{agent_loop, LoopStats};
use crate::agent::AgentId;
use crate::feature_hooks::Hook;
use std::time::Instant;
static WAKE: Hook<fn()> = Hook::empty();
static FAST: Hook<fn()> = Hook::empty();
static SETTLE: Hook<fn()> = Hook::empty();
static SHUTDOWN: Hook<fn()> = Hook::empty();
static PARK: Hook<fn() -> bool> = Hook::empty();
static OUTSTANDING: Hook<fn() -> bool> = Hook::empty();
static CLAIM: Hook<fn() -> bool> = Hook::empty();
static NET_AVAILABLE: Hook<fn() -> bool> = Hook::empty();
static ARM: Hook<fn(Option<Instant>)> = Hook::empty();
static NOTIFY: Hook<fn(AgentId) -> bool> = Hook::empty();
static HAS_ROUTE: Hook<fn(AgentId) -> bool> = Hook::empty();
static STATS: Hook<fn() -> Option<LoopStats>> = Hook::empty();
static BOUNDED: Hook<fn(u64)> = Hook::empty();
pub(crate) fn install() {
    WAKE.set(agent_loop::wake_parked_agents);
    FAST.set(agent_loop::fast_turn);
    SETTLE.set(agent_loop::settle_turn);
    SHUTDOWN.set(agent_loop::shutdown_current_thread);
    PARK.set(park_installed);
    OUTSTANDING.set(agent_loop::has_outstanding_work);
    CLAIM.set(agent_loop::claim_route_for_wakes);
    NET_AVAILABLE.set(agent_loop::net_available);
    ARM.set(agent_loop::arm_timer);
    NOTIFY.set(agent_loop::notify_agent);
    HAS_ROUTE.set(agent_loop::has_route);
    STATS.set(agent_loop::loop_statistics);
    BOUNDED.set(agent_loop::turn_bounded);
}
fn park_installed() -> bool {
    agent_loop::eligible() && super::precise_wait::park()
}
fn call(slot: &Hook<fn()>) {
    if let Some(f) = slot.get() {
        f();
    }
}
pub(super) fn wake() {
    call(&WAKE);
}
pub(super) fn fast() {
    call(&FAST);
}
pub(super) fn settle() {
    call(&SETTLE);
}
pub(super) fn shutdown() {
    call(&SHUTDOWN);
}
pub(super) fn park() -> bool {
    PARK.get().is_some_and(|f| f())
}
pub(super) fn has_outstanding_work() -> bool {
    OUTSTANDING.get().is_some_and(|f| f())
}
pub(super) fn claim_route() -> bool {
    CLAIM.get().is_some_and(|f| f())
}
pub(super) fn net_available() -> bool {
    NET_AVAILABLE.get().is_some_and(|f| f())
}
pub(crate) fn arm_timer(at: Option<Instant>) {
    if let Some(arm) = ARM.get() {
        arm(at);
    }
}
pub(super) fn notify_agent(agent: AgentId) -> bool {
    NOTIFY.get().is_some_and(|f| f(agent))
}
pub fn has_route(agent: AgentId) -> bool {
    HAS_ROUTE.get().is_some_and(|f| f(agent))
}
pub fn loop_statistics() -> Option<LoopStats> {
    STATS.get().and_then(|f| f())
}
pub(super) fn turn_bounded(budget: u64) {
    if let Some(turn) = BOUNDED.get() {
        turn(budget);
    }
}
