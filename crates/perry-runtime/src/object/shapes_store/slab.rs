use super::*;

impl ShapeSlab {
    /// A slab that publishes nothing (tests).
    pub(in super::super) fn new() -> Self {
        ShapeSlab {
            pages: Vec::new(),
            dict_pages: Vec::new(),
            exotic_pages: Vec::new(),
            region: flat::Region::new(),
            static_rows: crate::codegen_abi::NATIVE_LAYOUT_ROWS,
            next_row: crate::codegen_abi::NATIVE_LAYOUT_ROWS,
            row_bound: crate::codegen_abi::NATIVE_LAYOUT_ROWS,
            chunks: OwnedChunks::new(),
            chunk_end: 0,
            len: 0,
            agent: false,
            protos: Default::default(),
        }
    }

    /// The agent's slab: the one [`Self::agent_record`] reads. One per
    /// agent (the runtime state's shape table).
    pub(in super::super) fn new_agent() -> Self {
        let mut slab = Self::new();
        let image = crate::object::class_image::layout_image();
        crate::object::class_image::publish_agent_layout_image(image);
        // The dynamic base is one process-wide value; an image's static rows
        // lie below it, so images with different S never share a chunk row.
        assert!(
            image.static_rows <= DYNAMIC_BASE_ROWS,
            "layout image has more static rows than the process dynamic base"
        );
        slab.region
            .commit_static(image.static_rows as usize)
            .unwrap_or_else(|error| panic!("cannot commit the static shape rows: {error:?}"));
        slab.static_rows = image.static_rows;
        slab.next_row = image.static_rows;
        slab.row_bound = image.static_rows;
        slab.agent = true;
        slab.publish_dir();
        slab.protos.make_agent();
        slab
    }

    #[inline]
    pub(super) fn id_of(band: u8, index: usize) -> u32 {
        match band {
            0 => SHAPE_ID_BASE + index as u32,
            1 => SITE_SHAPE_ID_END + index as u32,
            _ => EXOTIC_SHAPE_ID_BASE + index as u32,
        }
    }

    #[inline]
    pub(super) fn dir(&self, band: u8) -> &Vec<Page> {
        match band {
            0 => &self.pages,
            1 => &self.dict_pages,
            _ => &self.exotic_pages,
        }
    }

    #[inline]
    pub(super) fn dir_mut(&mut self, band: u8) -> &mut Vec<Page> {
        match band {
            0 => &mut self.pages,
            1 => &mut self.dict_pages,
            _ => &mut self.exotic_pages,
        }
    }

    /// `(page, chunk within page, record within chunk)` of a slab index.
    #[inline(always)]
    pub(super) fn split(index: usize) -> (usize, usize, usize) {
        (
            index >> (CHUNK_SHIFT + PAGE_SHIFT),
            (index >> CHUNK_SHIFT) & PAGE_MASK,
            index & CHUNK_MASK,
        )
    }

    /// The cell `index` names in a directory of `len` pages at `pages`: two
    /// dependent loads after the directory bound check. A page past it is the shared
    /// empty page, and an absent page or chunk is the shared empty one.
    ///
    /// # Safety
    /// `pages` holds `len` live entries (or `len` is 0).
    #[inline(always)]
    pub(super) unsafe fn walk(pages: *const Page, len: usize, index: usize) -> *mut ShapeRecord {
        let (page, chunk, slot) = Self::split(index);
        if page >= len {
            return std::ptr::null_mut();
        }
        let page = (*pages.add(page)).0.as_ref();
        let chunk = page[chunk].0.as_ref();
        *chunk[slot].get()
    }

    /// Present records.
    #[inline]
    pub(in super::super) fn len(&self) -> usize {
        self.len
    }

    /// The record for `id`, or `None` when the id names no descriptor in this
    /// slab. The pointer stays valid until the record is removed; a removal
    /// only ever happens through the table's own retirement paths.
    #[inline]
    pub(in super::super) fn record_ptr(&self, id: ShapeId) -> Option<*mut ShapeRecord> {
        let (band, index) = locate(id.0);
        if band >= 3 {
            return None;
        }
        let dir = self.dir(band as u8);
        std::ptr::NonNull::new(unsafe { Self::walk(dir.as_ptr(), dir.len(), index) })
            .map(|p| p.as_ptr())
    }

    /// A copy of the record for `id`.
    #[inline]
    #[cfg(test)]
    pub(in super::super) fn get(&self, id: ShapeId) -> Option<ShapeRecord> {
        // SAFETY: `record_ptr` only returns a cell of a live chunk.
        self.record_ptr(id).map(|p| unsafe { *p })
    }

    /// Lift `id` to the by-value descriptor.
    #[inline]
    #[cfg(test)]
    pub(in super::super) fn lift(&self, id: ShapeId) -> Option<ShapeDescriptor> {
        // SAFETY: as in `get`.
        self.record_ptr(id).map(|p| unsafe { (*p).lift(p) })
    }

    /// Install `record` under `id`, allocating the page and chunk on first
    /// touch. Returns the record it replaced, if the id was already present.
    #[cfg(test)]
    pub(in super::super) fn insert(
        &mut self,
        id: ShapeId,
        record: ShapeRecord,
    ) -> Option<ShapeRecord> {
        // Identity-boundary compatibility. Receivers never enter here.
        if let Some(ptr) = self.record_ptr(id) {
            let previous = unsafe { *ptr };
            self.write_record(ptr, id.0, previous.layout_word, record);
            Some(previous)
        } else {
            self.publish_dynamic(id, record)
                .expect("shape publication failed");
            None
        }
    }

    #[inline(always)]
    pub(super) fn write_record(
        &mut self,
        ptr: *mut ShapeRecord,
        id: u32,
        word: LayoutWord,
        mut record: ShapeRecord,
    ) {
        record.proto_cell = self
            .protos
            .identity_slot_ensure(record.proto_id)
            .map_or(0, |slot| slot as usize as u64);
        record.set(RECORD_FLAG_PRESENT, true);
        record.shape_id = ShapeId(id);
        record.layout_word = word;
        record.refresh_positional();
        record.refresh_symbol_presence();
        record.refresh_element_kind();
        record.probe = 0;
        unsafe {
            crate::object::layout_probe::mint_colour_masks(&mut record);
        }
        unsafe {
            *ptr = record;
        }
    }

    pub(in super::super) fn publish_static(
        &mut self,
        identity: ShapeId,
        requested: LayoutWord,
        mut record: ShapeRecord,
    ) -> Result<PublishedShape, PublishError> {
        if requested.row().0 >= self.static_rows {
            return Err(PublishError::InvalidWord);
        }
        if Self::word_for(identity, requested.row().0, &record)? != requested {
            return Err(PublishError::InvalidWord);
        }
        record.set(RECORD_FLAG_EXTERNAL_CARRIER, true);
        self.publish(identity, requested, record)
    }

    pub(in super::super) fn publish_dynamic(
        &mut self,
        identity: ShapeId,
        record: ShapeRecord,
    ) -> Result<PublishedShape, PublishError> {
        if self.next_row >= self.chunk_end {
            let start = CHUNKS.claim(self.region.capacity())?;
            self.chunks.push(start);
            self.next_row = start;
            self.chunk_end = start + DYNAMIC_CHUNK_ROWS;
        }
        // A present identity always carries a different row than the fresh one
        // chosen here, so `publish` reports it as a conflict.
        let word = Self::word_for(identity, self.next_row, &record)?;
        let published = self.publish(identity, word, record)?;
        self.next_row += 1;
        self.row_bound = self.row_bound.max(self.next_row);
        if self.agent {
            // The directory buffers were published by `publish` when they
            // moved; only the receivers' row bound grows with every mint.
            AGENT_SHAPE_DIR[0].flat_ids.set(self.row_bound as usize);
        }
        Ok(published)
    }

    pub(super) fn word_for(
        identity: ShapeId,
        row: u32,
        record: &ShapeRecord,
    ) -> Result<LayoutWord, PublishError> {
        let (band, _) = locate(identity.0);
        if band >= 3 || !super::super::shape_id_kind_admits(identity.0, record.proto_id) {
            return Err(PublishError::InvalidWord);
        }
        LayoutWord::new(
            LayoutRow(row),
            band as u32,
            super::super::proto_id_kind(record.proto_id),
        )
        .ok_or(PublishError::Capacity)
    }

    #[inline(always)]
    pub(super) fn publish(
        &mut self,
        identity: ShapeId,
        word: LayoutWord,
        record: ShapeRecord,
    ) -> Result<PublishedShape, PublishError> {
        if let Some(p) = self.record_ptr(identity) {
            let existing = unsafe { &*p };
            let agrees = existing.layout_word == word
                && existing.integrity_flags() == record.integrity_flags()
                && existing.facts_match_proto_with_special(
                    // An empty key list is the same fact whether the request
                    // carries no keys array (a deferred-width class birth) or
                    // the canonical empty one: one identity may be requested
                    // from both.
                    if record.logical_key_count == 0 && existing.logical_key_count == 0 {
                        existing.keys
                    } else {
                        record.keys
                    },
                    record.logical_key_count,
                    record.live_inline_slot_count,
                    record.semantic_generation,
                    record.object_kind(),
                    record.hole_count,
                    record.proto_id,
                    record.summary(),
                    record.rep,
                    record.constfn_infos(),
                    record.brands(),
                );
            unsafe {
                record.release_extras();
            }
            return if agrees {
                Ok(PublishedShape { identity, word })
            } else {
                Err(PublishError::IdentityConflict)
            };
        }
        let row = word.row().0 as usize;
        let ptr = self.region.prepare(row)?;
        if unsafe { (*ptr).present() } {
            return Err(PublishError::RowConflict);
        }
        self.write_record(ptr, identity.0, word, record);
        // Leaves are boxed in place: the commit view moves only with the top
        // vector.
        if self.region.published(row) {
            self.publish_commit();
        }
        let (band, index) = locate(identity.0);
        let (page, chunk, slot) = Self::split(index);
        let dir = self.dir_mut(band as u8);
        let resized = page >= dir.len();
        if resized {
            dir.resize_with(page + 1, Slot::empty);
        }
        *dir[page].owned_or_alloc()[chunk].owned_or_alloc()[slot].get_mut() = ptr;
        self.len += 1;
        if resized {
            self.publish_dir();
        }
        Ok(PublishedShape { identity, word })
    }

    /// Requires a live carrier; bounds alone do not authorize a Region read.
    pub(in super::super) unsafe fn record_for_handle(&self, word: LayoutWord) -> *mut ShapeRecord {
        self.region.base().add(word.row().0 as usize)
    }

    #[cfg(test)]
    pub(in super::super) fn handle_by_identity(&self, identity: ShapeId) -> Option<LayoutWord> {
        self.record_ptr(identity)
            .map(|p| unsafe { (*p).layout_word })
    }

    /// Clear the record under `id`, returning it if it was present.
    pub(in super::super) fn remove(&mut self, id: ShapeId) -> Option<ShapeRecord> {
        let (band, index) = locate(id.0);
        if band >= 3 {
            return None;
        }
        let (page, chunk, slot) = Self::split(index);
        let link = self.dir_mut(band as u8).get_mut(page)?.owned_mut()?[chunk].owned_mut()?[slot]
            .get_mut();
        let p = std::ptr::NonNull::new(*link)?.as_ptr();
        // Membership disappears before the Region can be discarded.
        *link = std::ptr::null_mut();
        let mut previous = unsafe { *p };
        crate::object::layout_probe::release_probe(previous.probe);
        previous.probe = 0;
        unsafe {
            *p = ShapeRecord::EMPTY;
        }
        self.len -= 1;
        self.region.retired(previous.layout_word.row().0 as usize);
        Some(previous)
    }

    /// Visit every present record in id order. The callback may write
    /// through the record pointer; it must not insert or remove.
    pub(in super::super) fn for_each(&self, mut f: impl FnMut(ShapeId, *mut ShapeRecord)) {
        for dict in [0u8, 1, 2] {
            for (page_index, page) in self.dir(dict).iter().enumerate() {
                let Some(page) = page.owned() else {
                    continue;
                };
                for (chunk_index, chunk) in page.iter().enumerate() {
                    let Some(chunk) = chunk.owned() else {
                        continue;
                    };
                    let base = ((page_index << PAGE_SHIFT) | chunk_index) << CHUNK_SHIFT;
                    for (slot, cell) in chunk.iter().enumerate() {
                        let p = unsafe { *cell.get() };
                        if !p.is_null() {
                            f(ShapeId(Self::id_of(dict, base | slot)), p);
                        }
                    }
                }
            }
        }
    }

    /// Every present id, in id order.
    #[cfg(test)]
    pub(in super::super) fn ids(&self) -> Vec<ShapeId> {
        let mut ids = Vec::with_capacity(self.len);
        self.for_each(|id, _| ids.push(id));
        ids
    }

    /// Free chunks that hold no present record, and pages that hold no
    /// chunk. Called once per major collection, after dead-key pruning:
    /// retirement is monotonic in id order for the common workload, so the
    /// oldest chunks empty first.
    pub(in super::super) fn release_empty_chunks(&mut self) {
        for band in 0..3 {
            let dir = self.dir_mut(band);
            for page in dir.iter_mut() {
                if let Some(chunks) = page.owned_mut() {
                    for chunk in chunks.iter_mut() {
                        if chunk
                            .owned()
                            .is_some_and(|c| c.iter().all(|p| unsafe { (*p.get()).is_null() }))
                        {
                            chunk.release();
                        }
                    }
                    if chunks.iter().all(|c| c.is_shared()) {
                        page.release();
                    }
                }
            }
            while dir.last().is_some_and(|p| p.is_shared()) {
                dir.pop();
            }
            dir.shrink_to_fit();
        }
        self.publish_dir();
    }

    /// Publish every band's page vector into [`AGENT_SHAPE_DIR`], if this is
    /// the agent's slab. Called after every change to a page vector (a
    /// resize or shrink moves its buffer); a page or chunk allocated or
    /// released in place is visible through the published buffer already.
    #[inline]
    pub(super) fn publish_commit(&self) {
        if self.agent {
            let (tops, len, shift) = self.region.commit_view();
            AGENT_COMMIT.0.set(tops);
            AGENT_COMMIT.1.set(len);
            AGENT_COMMIT.2.set(shift);
        }
    }

    pub(super) fn publish_dir(&self) {
        if !self.agent {
            return;
        }
        self.publish_commit();
        for (band, dir) in [&self.pages, &self.dict_pages, &self.exotic_pages]
            .into_iter()
            .enumerate()
        {
            AGENT_SHAPE_DIR[band].pages.set(dir.as_ptr());
            AGENT_SHAPE_DIR[band].len.set(dir.len());
        }
        AGENT_SHAPE_DIR[0].flat_base.set(self.region.base());
        AGENT_SHAPE_DIR[0].flat_ids.set(self.row_bound as usize);
        AGENT_SHAPE_DIR[0]
            .proto_cell
            .set(crate::array::object_prototype_cell());
        AGENT_SHAPE_DIR[0]
            .word_dir
            .set(super::super::shapes_prototype::agent_word_dir_addr());
        // Emitted read sites hand the ordinary entry's address to the miss
        // front from the agent's pointer block; publish it with the directory.
        crate::agent_ptrs::publish(
            crate::agent_ptrs::AGENT_PTR_SHAPE_DIR,
            Self::ordinary_dir_addr(),
        );
    }

    /// The address of THIS thread's ordinary-band directory
    /// (`AGENT_SHAPE_DIR[0]`), as an opaque pointer for
    /// [`Self::ordinary_record_in`]. Stable for the thread's life (a
    /// const-initialised `#[thread_local]` with no destructor), so the agent
    /// publishes it into its `PERRY_AGENT_PTRS` slot and emitted code hands it
    /// to the megamorphic read confirm, which then reads no thread-local at
    /// all (a runtime thread-local access is a `__tls_get_addr` call on ELF
    /// and a TLV thunk call on Darwin, which would give the stub a frame).
    #[inline]
    pub(crate) fn ordinary_dir_addr() -> *const u8 {
        &AGENT_SHAPE_DIR[0] as *const BandDir as *const u8
    }

    /// The record of ShapeId `id` in the ordinary directory at `dir` (this
    /// thread's [`Self::ordinary_dir_addr`], or `PERRY_EMPTY_SHAPE_DIR`), or
    /// `None` past its pages: the ordinary band of [`Self::agent_record`], for
    /// the megamorphic read, with no band select and no thread-local access.
    /// Any other id (dictionary, exotic, not a ShapeId) indexes past the
    /// ordinary directory, whose pages stop below the dictionary band. Inside
    /// the pages the record may be absent (the shared `ShapeRecord::EMPTY`):
    /// its position bound is 0, the confirm's answer for it anyway.
    ///
    /// The page bound is a branch here, not [`Self::walk`]'s select: the
    /// megamorphic read's ids are the receivers' own, all inside the pages,
    /// so the branch is predicted and costs less than the select's address
    /// arithmetic (lead_mega1: 138.4 instructions/read with it, 139.9 with the
    /// select).
    ///
    /// # Safety
    /// `dir` is this thread's [`Self::ordinary_dir_addr`] or
    /// `PERRY_EMPTY_SHAPE_DIR`; never null.
    #[inline(always)]
    pub(in super::super) unsafe fn ordinary_record_in<'a>(
        dir: *const u8,
        word: LayoutWord,
    ) -> Option<&'a ShapeRecord> {
        let dir = &*(dir as *const BandDir);
        let row = LayoutWord(word.0).ordinary_row(dir.flat_ids.get())?;
        Some(&*dir.flat_base.get().add(row.0 as usize))
    }

    /// The record `id` names in THIS agent's slab — never null. An id that
    /// names no record here (never minted, retired, not a ShapeId at all, or
    /// read before the agent built its runtime state) reads the shared
    /// `ShapeRecord::EMPTY`: not present, position bound 0, every lane `Any`.
    /// A caller whose answer for "no record" is exactly that reads the fields
    /// with no presence test; any other caller tests `present()`.
    ///
    /// The whole lookup: one thread-local load of the band's directory, the
    /// band select, and two dependent loads (page, chunk). No `state()`.
    ///
    /// The pointer must not be written unless the record is present (the
    /// shared empty is never written), and stays valid until the record is
    /// removed.
    #[inline(always)]
    pub(in super::super) fn agent_record(id: ShapeId) -> *mut ShapeRecord {
        let rel = id.0.wrapping_sub(SHAPE_ID_BASE);
        let band = band_of(rel);
        let dir = &AGENT_SHAPE_DIR[band];
        let index = rel.wrapping_sub(dir.base_rel) as usize;
        let ptr = unsafe { Self::walk(dir.pages.get(), dir.len.get(), index) };
        if ptr.is_null() {
            std::ptr::addr_of!(EMPTY_RECORD.0).cast_mut()
        } else {
            ptr
        }
    }

    /// The record of a held word (memo, record link, parameter): checked
    /// against this agent's issued rows, so a retired, unpublished or
    /// other-agent word answers absent: its row reads the zero (absent)
    /// record of a readable reservation, or on Windows, where a reservation
    /// cannot be read, the page bookkeeping answers first.
    pub(in super::super) fn agent_record_for_word(word: LayoutWord) -> *mut ShapeRecord {
        Self::agent_record_for_word_present(word)
            .unwrap_or(std::ptr::addr_of!(EMPTY_RECORD.0).cast_mut())
    }
    #[inline]
    pub(in super::super) fn agent_record_for_word_present(
        word: LayoutWord,
    ) -> Option<*mut ShapeRecord> {
        if word.0 & crate::codegen_abi::LAYOUT_TAG == 0 {
            return None;
        }
        // The checked read of a held word, answered from this thread's
        // published directory (the same facts `handle_record_ptr` proves):
        // tag, band, row below the issued bound, record present (and, where
        // the reservation is not readable, row committed).
        let dir = &AGENT_SHAPE_DIR[0];
        let base = dir.flat_base.get();
        if base.is_null() {
            return Self::unpublished_record_for_word((word).0);
        }
        let w = word;
        if w.band() >= 3 {
            return None;
        }
        let row = w.row().0 as usize;
        if row >= dir.flat_ids.get()
            || !flat::RESERVATION_READABLE
                && !unsafe {
                    flat::committed_in(
                        AGENT_COMMIT.0.get(),
                        AGENT_COMMIT.1.get(),
                        AGENT_COMMIT.2.get(),
                        row,
                    )
                }
        {
            return None;
        }
        let p = unsafe { base.add(row) };
        unsafe { (*p).present() }.then_some(p)
    }

    /// A slab that publishes nothing (tests): the slab answers.
    #[cold]
    #[inline(never)]
    pub(super) fn unpublished_record_for_word(word: u32) -> Option<*mut ShapeRecord> {
        crate::state::state()
            .shapes
            .slab()
            .handle_record_ptr(LayoutWord(word))
    }

    /// A live receiver's own header word, or zero for an unshaped receiver.
    /// The receiver is the carrier, so the unchecked read is sound; never use
    /// this for a word read from a memo, a record or a parameter.
    #[inline(always)]
    pub(in super::super) fn agent_record_for_receiver(word: u32) -> *mut ShapeRecord {
        Self::receiver_row_record(word).unwrap_or(std::ptr::addr_of!(EMPTY_RECORD.0).cast_mut())
    }
    #[inline(always)]
    pub(in super::super) fn agent_record_for_receiver_present(
        word: u32,
    ) -> Option<*mut ShapeRecord> {
        let p = Self::receiver_row_record(word)?;
        unsafe { (*p).present() }.then_some(p)
    }
    /// The row cell a receiver word names, if it is an issued row. A layout
    /// word is tagged and in band 0..=2. With the tag flipped and the shadow
    /// and kind bits cleared, a band-0 word is its row, and rows are below
    /// `1 << LAYOUT_BAND_SHIFT`: so an ordinary receiver (the common one) is a
    /// single compare against the row bound, and an untagged word reads as at
    /// least `LAYOUT_TAG` and fails it. Bands 1 and 2 (dictionary, exotic)
    /// take the second test; band 3 is no layout word and fails it.
    #[inline(always)]
    pub(super) fn receiver_row_record(word: u32) -> Option<*mut ShapeRecord> {
        use crate::codegen_abi::{LAYOUT_KIND_MASK, LAYOUT_SHADOW_BIT, LAYOUT_TAG};
        let dir = &AGENT_SHAPE_DIR[0];
        let bound = dir.flat_ids.get();
        let band0 = ((word ^ LAYOUT_TAG) & !(LAYOUT_SHADOW_BIT | LAYOUT_KIND_MASK)) as usize;
        if band0 >= bound {
            return Self::receiver_row_record_banded(word, bound);
        }
        let row = band0;
        // SAFETY: an issued row of this agent's readable reservation.
        let record = unsafe { dir.flat_base.get().add(row) };
        if !unsafe { (*record).present() } {
            crate::object::layout_desc::publish_image_receiver(word);
        }
        Some(record)
    }
    /// [`Self::receiver_row_record`] for a word outside band 0 (dictionary,
    /// exotic) or no issued row: out of line, so the inlined ordinary read
    /// stays one compare.
    #[cold]
    #[inline(never)]
    pub(super) fn receiver_row_record_banded(word: u32, bound: usize) -> Option<*mut ShapeRecord> {
        use crate::codegen_abi::{
            LAYOUT_BAND_SHIFT, LAYOUT_ROW_MASK, LAYOUT_SHADOW_BIT, LAYOUT_TAG,
        };
        let w = word & !LAYOUT_SHADOW_BIT;
        if w.wrapping_sub(LAYOUT_TAG) >= 3 << LAYOUT_BAND_SHIFT {
            return None;
        }
        let row = (w & LAYOUT_ROW_MASK) as usize;
        if row >= bound {
            return None;
        }
        // SAFETY: an issued row of this agent's readable reservation.
        let record = unsafe { AGENT_SHAPE_DIR[0].flat_base.get().add(row) };
        if !unsafe { (*record).present() } {
            crate::object::layout_desc::publish_image_receiver(word);
        }
        Some(record)
    }
    /// Requires an existing owner/carrier; never use for identity absence checks.
    pub(in super::super) fn handle_record_ptr(&self, word: LayoutWord) -> Option<*mut ShapeRecord> {
        if word.0 & crate::codegen_abi::LAYOUT_TAG == 0
            || word.band() >= 3
            || word.row().0 >= self.row_bound
            || !flat::RESERVATION_READABLE && !self.region.row_committed(word.row().0 as usize)
        {
            return None;
        }
        let p = unsafe { self.record_for_handle(word) };
        unsafe { (*p).present() }.then_some(p)
    }
    #[cfg(test)]
    pub(in super::super) fn region_row_committed(&self, row: usize) -> bool {
        self.region.row_committed(row)
    }
    #[cfg(test)]
    pub(in super::super) fn region_rows_per_page(&self) -> usize {
        self.region.page / core::mem::size_of::<ShapeRecord>()
    }
    pub(in super::super) fn get_handle(&self, word: LayoutWord) -> Option<ShapeRecord> {
        self.handle_record_ptr(word).map(|p| unsafe { *p })
    }
    pub(in super::super) fn lift_handle(&self, word: LayoutWord) -> Option<ShapeDescriptor> {
        self.handle_record_ptr(word)
            .map(|p| unsafe { (*p).lift(p) })
    }
    pub(in super::super) fn remove_handle(&mut self, word: LayoutWord) -> Option<ShapeRecord> {
        let p = self.handle_record_ptr(word)?;
        self.remove(unsafe { (*p).shape_id })
    }
    pub(in super::super) fn for_each_handle(
        &self,
        mut f: impl FnMut(LayoutWord, *mut ShapeRecord),
    ) {
        self.for_each(|_, p| f(unsafe { (*p).layout_word }, p));
    }

    /// [`Self::agent_record`], or `None` when the record is absent.
    #[inline(always)]
    pub(in super::super) fn agent_record_present(id: ShapeId) -> Option<*mut ShapeRecord> {
        let record = Self::agent_record(id);
        // SAFETY: `agent_record` never returns null or a dead cell.
        unsafe { (*record).present() }.then_some(record)
    }

    #[cfg(test)]
    pub(in super::super) fn clear(&mut self) {
        let ids = self.ids();
        for id in ids {
            if let Some(r) = self.remove(id) {
                unsafe {
                    r.release_extras();
                }
            }
        }
        for band in 0..3 {
            Self::free_dir(self.dir_mut(band), band);
        }
        // Issued dynamic rows are never reused, including across test clears.
        self.publish_dir();
        self.protos.reset();
    }

    /// Bytes held: the page directory, every allocated page and every
    /// allocated chunk.
    pub(in super::super) fn estimated_bytes(&self) -> usize {
        let mut pages = 0usize;
        let mut chunks = 0usize;
        for page in self
            .pages
            .iter()
            .chain(self.dict_pages.iter())
            .chain(self.exotic_pages.iter())
            .filter_map(Slot::owned)
        {
            pages += 1;
            chunks += page.iter().filter(|c| !c.is_shared()).count();
        }
        (self.pages.capacity() + self.dict_pages.capacity() + self.exotic_pages.capacity())
            * std::mem::size_of::<Page>()
            + pages * PAGE_LEN * std::mem::size_of::<Slot<ChunkCells>>()
            + chunks * CHUNK_LEN * std::mem::size_of::<*mut ShapeRecord>()
            + self.region.committed_bytes()
    }

    /// Allocated chunks (diagnostics).
    #[cfg(test)]
    pub(in super::super) fn chunk_count(&self) -> usize {
        self.pages
            .iter()
            .chain(self.dict_pages.iter())
            .chain(self.exotic_pages.iter())
            .filter_map(Slot::owned)
            .map(|page| page.iter().filter(|c| !c.is_shared()).count())
            .sum()
    }

    /// Free every page and chunk a directory owns, and empty it.
    pub(super) fn free_dir(dir: &mut Vec<Page>, _band: u8) {
        for page in dir.iter_mut() {
            if let Some(chunks) = page.owned_mut() {
                for chunk in chunks.iter_mut() {
                    chunk.release();
                }
            }
            page.release();
        }
        dir.clear();
    }
}
