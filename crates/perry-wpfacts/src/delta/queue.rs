//! Paged storage for live deliveries. Node headers link only their pending events.
use super::{Batch, Delta, Event, Values};
const PAGE: usize = (4096 - 2 * std::mem::size_of::<u32>()) / std::mem::size_of::<Slot>();
#[derive(Clone, Debug, Default)]
pub struct Pending {
    head: u32,
    tail: u32,
    len: usize,
}
impl Pending {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
#[derive(Clone, Debug)]
struct Slot {
    event: Option<Event>,
    next: u32,
}
#[derive(Clone, Debug)]
struct Page {
    slots: [Slot; PAGE],
    free: u32,
    live: u32,
}
impl Page {
    fn new() -> Self {
        Self {
            slots: std::array::from_fn(|i| Slot {
                event: None,
                next: if i + 1 == PAGE { 0 } else { i as u32 + 2 },
            }),
            free: 1,
            live: 0,
        }
    }
}
#[derive(Clone, Debug, Default)]
struct PageEntry {
    page: Option<Box<Page>>,
    prev: u32,
    next: u32,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct EventQueue {
    pages: Vec<PageEntry>,
    available: u32,
    free_page: u32,
    live: usize,
}
impl EventQueue {
    #[cfg(test)]
    fn slot(&self, id: u32) -> &Slot {
        let i = id as usize - 1;
        &self.pages[i / PAGE].page.as_ref().unwrap().slots[i % PAGE]
    }
    fn slot_mut(&mut self, id: u32) -> &mut Slot {
        let i = id as usize - 1;
        &mut self.pages[i / PAGE].page.as_mut().unwrap().slots[i % PAGE]
    }
    fn unlink(&mut self, id: u32) {
        let e = &self.pages[id as usize - 1];
        let (prev, next) = (e.prev, e.next);
        if prev == 0 {
            self.available = next;
        } else {
            self.pages[prev as usize - 1].next = next;
        }
        if next != 0 {
            self.pages[next as usize - 1].prev = prev;
        }
        let e = &mut self.pages[id as usize - 1];
        e.prev = 0;
        e.next = 0;
    }
    fn available(&mut self, id: u32) {
        let next = self.available;
        self.pages[id as usize - 1].next = next;
        if next != 0 {
            self.pages[next as usize - 1].prev = id;
        }
        self.available = id;
    }
    fn allocate(&mut self, event: Event) -> u32 {
        if self.available == 0 {
            let id = if self.free_page != 0 {
                let id = self.free_page;
                self.free_page = self.pages[id as usize - 1].next;
                self.pages[id as usize - 1] = PageEntry::default();
                id
            } else {
                assert!(self.pages.len() < (u32::MAX as usize / PAGE));
                self.pages.push(PageEntry::default());
                self.pages.len() as u32
            };
            self.pages[id as usize - 1].page = Some(Box::new(Page::new()));
            self.available(id);
        }
        let id = self.available;
        let page = self.pages[id as usize - 1].page.as_mut().unwrap();
        let offset = page.free - 1;
        let slot = &mut page.slots[offset as usize];
        page.free = slot.next;
        slot.next = 0;
        slot.event = Some(event);
        page.live += 1;
        if page.free == 0 {
            self.unlink(id);
        }
        self.live += 1;
        (id - 1) * PAGE as u32 + offset + 1
    }
    fn release(&mut self, id: u32) -> (Event, u32) {
        let i = id as usize - 1;
        let page_id = (i / PAGE + 1) as u32;
        let page = self.pages[i / PAGE].page.as_mut().unwrap();
        let full = page.free == 0;
        let slot = &mut page.slots[i % PAGE];
        let event = slot.event.take().unwrap();
        let next = slot.next;
        slot.next = page.free;
        page.free = (i % PAGE + 1) as u32;
        page.live -= 1;
        let empty = page.live == 0;
        if full {
            self.available(page_id);
        }
        if empty {
            self.unlink(page_id);
            let e = &mut self.pages[i / PAGE];
            e.page = None;
            e.next = self.free_page;
            self.free_page = page_id;
        }
        self.live -= 1;
        if self.live == 0 {
            self.pages = Vec::new();
            self.available = 0;
            self.free_page = 0;
        }
        (event, next)
    }
    fn append_event(&mut self, pending: &mut Pending, event: Event) {
        let id = self.allocate(event);
        if pending.tail == 0 {
            pending.head = id;
        } else {
            self.slot_mut(pending.tail).next = id;
        }
        pending.tail = id;
    }
    pub(crate) fn split_tail(&mut self, pending: &Pending) {
        if pending.tail != 0 {
            let last = self.slot_mut(pending.tail).event.as_mut().unwrap();
            if last.boundary > 16 {
                last.mergeable = false;
            }
        }
    }
    pub(crate) fn push_to(&mut self, pending: &mut Pending, t: u32, boundary: u32) {
        pending.len += 1;
        if pending.tail != 0 {
            let last = self.slot_mut(pending.tail).event.as_mut().unwrap();
            if last.mergeable && last.boundary == boundary {
                last.values.push(t);
                return;
            }
        }
        self.append_event(
            pending,
            Event {
                values: Values::Owned(vec![t]),
                boundary,
                had_targets: boundary != 0,
                mergeable: true,
            },
        );
    }
    pub(crate) fn append_to(&mut self, pending: &mut Pending, batch: &Batch, boundary: u32) -> u64 {
        if batch.len() == 0 {
            return 0;
        }
        pending.len += batch.len();
        if pending.tail != 0 {
            let last = self.slot_mut(pending.tail).event.as_mut().unwrap();
            if last.mergeable && last.boundary == boundary {
                return last.values.append(batch);
            }
        }
        self.append_event(
            pending,
            Event {
                values: Values::Shared(batch.0.clone()),
                boundary,
                had_targets: boundary != 0,
                mergeable: true,
            },
        );
        batch.0.chunks.len().saturating_sub(1) as u64
    }
    pub(crate) fn take(&mut self, pending: &mut Pending, out: &mut Delta) {
        debug_assert!(out.is_empty());
        let pending = std::mem::take(pending);
        out.len = pending.len;
        let mut id = pending.head;
        while id != 0 {
            let (event, next) = self.release(id);
            out.events.push(event);
            id = next;
        }
    }
    pub(crate) fn remap(&mut self, pending: &Pending, keep: &[bool]) {
        if pending.head == 0 {
            return;
        }
        let mut rank = Vec::with_capacity(keep.len() + 1);
        rank.push(0u32);
        for &kept in keep {
            rank.push(rank.last().unwrap() + u32::from(kept));
        }
        let mut id = pending.head;
        while id != 0 {
            let slot = self.slot_mut(id);
            slot.event.as_mut().unwrap().remap(&rank);
            id = slot.next;
        }
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.pages.capacity() * std::mem::size_of::<PageEntry>()
            + self.pages.iter().filter(|e| e.page.is_some()).count() * std::mem::size_of::<Page>()
    }
    #[cfg(test)]
    pub(crate) fn snapshot(&self, pending: &Pending) -> Delta {
        let mut out = Delta {
            events: Vec::new(),
            len: pending.len,
        };
        let mut id = pending.head;
        while id != 0 {
            let slot = self.slot(id);
            out.events.push(slot.event.as_ref().unwrap().clone());
            id = slot.next;
        }
        out
    }
}
