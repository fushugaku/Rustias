//! Original 24-slot allocation priority, SH3 007aac/007b4c and 0075b4/007610.
pub const VOICE_COUNT: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationOwner(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceClaim {
    pub owner: AllocationOwner,
    pub note_flags: u8,
    pub release_flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationPass {
    Reuse,
    Replace,
    Fresh,
}

impl VoiceClaim {
    pub fn priority(self, query: AllocationOwner, position: u8, pass: AllocationPass) -> u32 {
        let same = self.owner == query;
        let held = self.note_flags & 128 != 0;
        let flags = self.release_flags & 3;
        let position = ((position as u32) & 31) << 16;
        match pass {
            AllocationPass::Reuse => {
                let status = ((held as u32) << 30) | ((flags as u32) << 28);
                if same && status != 0 {
                    0x80000000 | position | status
                } else {
                    0
                }
            }
            AllocationPass::Replace => {
                if same {
                    0x80000000 | position | ((held as u32) << 30) | ((flags as u32) << 28)
                } else {
                    (position ^ 0x001f0000) | ((!held as u32) << 30) | (((flags ^ 3) as u32) << 28)
                }
            }
            AllocationPass::Fresh => {
                (position ^ 0x001f0000)
                    | ((!held as u32) << 31)
                    | (((flags & 2 == 0) as u32) << 30)
                    | (((flags & 1 == 0) as u32) << 29)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceOrder(pub [u8; VOICE_COUNT]);
impl Default for VoiceOrder {
    fn default() -> Self {
        Self(core::array::from_fn(|i| i as u8))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceSelection {
    pub slot: u8,
    pub position: u8,
    pub priority: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceCostTables {
    pub base: u16,
    pub primary: [u16; 64],
    pub secondary: [u16; 4],
    pub filters: [u16; 16],
    pub shaper: [u16; 18],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceCostParameters {
    pub primary: u8,
    pub secondary: u8,
    pub filter_route: u8,
    pub drive_mode: u8,
    pub shaper_type: u8,
}
impl VoiceCostTables {
    pub fn cost(&self, p: VoiceCostParameters) -> Option<u32> {
        let primary = ((p.primary & 0x30) >> 4) | ((p.primary & 15) << 2);
        let filter = ((p.filter_route & 0x30) >> 4) | ((p.filter_route & 3) << 2);
        let shaper = if p.drive_mode & 3 == 2 {
            (p.shaper_type & 15) + 2
        } else {
            p.drive_mode & 3
        };
        Some(
            self.base as u32
                + *self.primary.get(primary as usize)? as u32
                + self.secondary[(p.secondary & 3) as usize] as u32
                + self.filters[filter as usize] as u32
                + self.shaper[shaper as usize] as u32,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessorBudget {
    pub costs: [u16; VOICE_COUNT],
    pub master_overhead: u16,
}

/// Selection, queue order and processor costs are owned by one instrument.
/// Controller envelope flags remain explicit; note events alone cannot infer
/// all the firmware's release/acknowledgement states.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceAllocator {
    pub order: VoiceOrder,
    pub claims: [VoiceClaim; VOICE_COUNT],
    pub budget: ProcessorBudget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceAssignment {
    pub slot: u8,
    pub displaced: u32,
}

impl Default for ProcessorBudget {
    fn default() -> Self {
        Self {
            costs: [0; VOICE_COUNT],
            master_overhead: 0,
        }
    }
}

impl VoiceAllocator {
    /// 007708 compares remaining processor cost after the selected actor's
    /// unsigned old cost is replaced. Its 008f04 reclaim variant compares
    /// remaining cost with the processor mask (one/two), unlike 008e0c's
    /// cost threshold; preserve that original register reuse.
    pub fn prepare_mono_budget(&mut self, slot: usize, cost: u16) -> u32 {
        self.prepare_mono_budget_excluding(slot, cost, 0)
    }
    fn prepare_mono_budget_excluding(&mut self, slot: usize, cost: u16, excluded: u32) -> u32 {
        let additional = cost as i32 - self.budget.costs[slot] as i32;
        let available = self.budget.remaining()[slot / 12].wrapping_sub(additional);
        if available > cost as i32 {
            0
        } else {
            self.reclaim_ordered(None, 1 << (slot / 12), excluded).1
        }
    }
    /// Mono first prefers a still active/releasing voice of this timbre,
    /// then uses the original replacement priority (007610/0075b4).
    pub fn allocate_mono(
        &mut self,
        owner: AllocationOwner,
        note: u8,
        cost: u16,
    ) -> Option<VoiceAssignment> {
        let selected = self
            .order
            .select(&self.claims, owner, 0, AllocationPass::Reuse)
            .or_else(|| {
                self.order
                    .select(&self.claims, owner, 0, AllocationPass::Replace)
            })?;
        let slot = selected.slot as usize;
        let mut displaced = if self.claims[slot].owner != owner
            && (self.claims[slot].note_flags & 128 != 0 || self.claims[slot].release_flags & 3 != 0)
        {
            1 << slot
        } else {
            0
        };
        self.order.move_to_back(selected.position);
        displaced |= self.prepare_mono_budget(slot, cost);
        self.budget.costs[slot] = cost;
        self.claims[slot] = VoiceClaim {
            owner,
            note_flags: (note & 127) | 128,
            release_flags: 0,
        };
        Some(VoiceAssignment {
            slot: selected.slot,
            displaced,
        })
    }
    /// 008542 visits held owner voices in physical order and moves each to
    /// the back. Its single-trigger update preserves release flags.
    pub fn retarget_mono(&mut self, owner: AllocationOwner, note: u8) -> u32 {
        let mut selected = 0;
        for slot in 0..VOICE_COUNT {
            if self.claims[slot].owner == owner && self.claims[slot].note_flags & 128 != 0 {
                let position = self
                    .order
                    .0
                    .iter()
                    .position(|&s| s as usize == slot)
                    .unwrap();
                self.order.move_to_back(position as u8);
                self.claims[slot].note_flags = (note & 127) | 128;
                selected |= 1 << slot;
            }
        }
        selected
    }
    /// 006fe8/008570 reuse every held owner actor. The original skips
    /// allocation/reclamation and republishes each actor's compiled cost.
    pub fn retrigger_mono(&mut self, owner: AllocationOwner, note: u8, cost: u16) -> u32 {
        let selected = self.retarget_mono(owner, note);
        for slot in 0..VOICE_COUNT {
            if selected & (1 << slot) != 0 {
                self.claims[slot].release_flags = 0;
                self.budget.costs[slot] = cost;
            }
        }
        selected
    }
    /// Original 008e0c's queue and cost policy, excluding host-side cleanup.
    /// The returned mask lets the application destroy each displaced graph.
    pub fn reclaim(&mut self, cost: i32, processors: u8) -> (u8, u32) {
        self.reclaim_ordered(Some(cost), processors, 0)
    }
    fn reclaim_ordered(&mut self, cost: Option<i32>, processors: u8, excluded: u32) -> (u8, u32) {
        let order = self.order;
        let result = self.reclaim_bookkeeping(cost, processors, excluded);
        // 009010/0093fc promotes each reclaimed actor past preceding held
        // actors after the complete flag/cost scan, in reclamation order.
        for slot in order.0 {
            if result.1 & (1 << slot) != 0 {
                self.order.promote_released(slot, &self.claims);
            }
        }
        result
    }
    fn reclaim_bookkeeping(
        &mut self,
        cost: Option<i32>,
        mut processors: u8,
        excluded: u32,
    ) -> (u8, u32) {
        let mut remaining = self.budget.remaining();
        let mut displaced = 0;
        for _ in 0..VOICE_COUNT {
            let victim = self.order.0.iter().copied().find(|&slot| {
                let c = self.claims[slot as usize];
                excluded & (1 << slot) == 0
                    && processors & (1 << (slot / 12)) != 0
                    && (c.release_flags & 3 != 0 || c.note_flags & 128 != 0)
            });
            let Some(slot) = victim else { break };
            let index = slot as usize;
            let processor = index / 12;
            self.claims[index].note_flags &= 127;
            self.claims[index].release_flags &= !3;
            displaced |= 1 << slot;
            // SH mov.w sign-extends the old declared cost in this routine.
            remaining[processor] =
                remaining[processor].wrapping_add(self.budget.costs[index] as i16 as i32);
            self.budget.costs[index] = 0;
            processors = 1 << processor;
            if remaining[processor] > cost.unwrap_or(processors as i32) {
                break;
            }
        }
        (processors, displaced)
    }

    pub fn allocate_poly(
        &mut self,
        owner: AllocationOwner,
        note: u8,
        cost: u16,
    ) -> Option<VoiceAssignment> {
        let mut processors = self.budget.available(cost as i32);
        let mut displaced = 0;
        if processors == 0 {
            (processors, displaced) = self.reclaim(cost as i32, 3);
        }
        let selected = self.order.select_with_processors(
            &self.claims,
            owner,
            0,
            AllocationPass::Fresh,
            processors,
        )?;
        let slot = selected.slot as usize;
        if self.claims[slot].note_flags & 128 != 0 || self.claims[slot].release_flags & 3 != 0 {
            displaced |= 1 << slot;
        }
        self.order.move_to_back(selected.position);
        self.budget.costs[slot] = cost;
        self.claims[slot] = VoiceClaim {
            owner,
            note_flags: (note & 127) | 128,
            release_flags: 0,
        };
        Some(VoiceAssignment {
            slot: selected.slot,
            displaced,
        })
    }

    /// Original007348 uses temporary held flags until the whole group is
    /// chosen; owner and release flags are published only at group commit.
    pub fn allocate_poly_group(
        &mut self,
        owner: AllocationOwner,
        timbre: u8,
        note: u8,
        cost: u16,
        layout: crate::voice_group::GroupLayout,
        slots: &mut crate::voice_group::VoiceGroupSlots,
    ) -> Option<crate::voice_group::GroupAssignment> {
        let mut selected_mask = 0;
        let mut displaced = 0;
        for index in 0..layout.count {
            let mut processors = self.budget.available(cost as i32);
            if processors == 0 {
                let reclaimed = self.reclaim(cost as i32, 3);
                processors = reclaimed.0;
                displaced |= reclaimed.1;
            }
            let Some(selection) = self.order.select_with_processors(
                &self.claims,
                owner,
                0,
                AllocationPass::Fresh,
                processors,
            ) else {
                break;
            };
            let slot = selection.slot as usize;
            if self.claims[slot].note_flags & 128 != 0 || self.claims[slot].release_flags & 3 != 0 {
                displaced |= 1 << slot;
            }
            self.order.move_to_back(selection.position);
            self.budget.costs[slot] = cost;
            self.claims[slot].note_flags |= 128;
            slots.indices[slot] = index;
            slots.stereo[slot] = if layout.stereo_pair {
                2u8.saturating_sub(index)
            } else {
                0
            };
            selected_mask |= 1 << slot;
        }
        self.commit_group(owner, timbre, note, cost, selected_mask, slots);
        (selected_mask != 0).then_some(crate::voice_group::GroupAssignment {
            selected: selected_mask,
            displaced,
            bank: layout.bank,
        })
    }

    /// Original007220 excludes actors already selected for this Mono group.
    /// Preserve their ordinal map only when reuse/previous-age tests allow it.
    #[allow(clippy::too_many_arguments)]
    pub fn allocate_mono_group(
        &mut self,
        owner: AllocationOwner,
        timbre: u8,
        note: u8,
        cost: u16,
        layout: crate::voice_group::GroupLayout,
        ages: &[u16; VOICE_COUNT],
        slots: &mut crate::voice_group::VoiceGroupSlots,
    ) -> Option<crate::voice_group::GroupAssignment> {
        let mut mask = 0;
        let mut displaced = 0;
        let mut reusable = 0;
        let mut replacements = 0;
        let mut prior_age = 0;
        for _ in 0..layout.count {
            let preferred = self
                .order
                .select(&self.claims, owner, mask, AllocationPass::Reuse);
            let selected = preferred.or_else(|| {
                self.order
                    .select(&self.claims, owner, mask, AllocationPass::Replace)
            })?;
            let slot = selected.slot as usize;
            self.order.move_to_back(selected.position);
            displaced |= self.prepare_mono_budget_excluding(slot, cost, mask);
            self.budget.costs[slot] = cost;
            if preferred.is_some() {
                reusable |= 1 << slot;
            } else {
                if self.claims[slot].note_flags & 128 != 0
                    || self.claims[slot].release_flags & 3 != 0
                {
                    displaced |= 1 << slot;
                }
                if slots.timbres[slot] == timbre {
                    if replacements == 0 {
                        prior_age = ages[slot];
                        reusable |= 1 << slot;
                    } else if ages[slot] == prior_age {
                        reusable |= 1 << slot;
                    }
                }
                replacements |= 1 << slot;
            }
            mask |= 1 << slot;
        }
        if reusable != mask {
            let mut ordinal = 0;
            for slot in 0..VOICE_COUNT {
                if mask & (1 << slot) != 0 {
                    slots.indices[slot] = ordinal;
                    slots.stereo[slot] = if layout.stereo_pair {
                        2u8.saturating_sub(ordinal)
                    } else {
                        0
                    };
                    ordinal += 1;
                }
            }
        }
        self.commit_group(owner, timbre, note, cost, mask, slots);
        (mask != 0).then_some(crate::voice_group::GroupAssignment {
            selected: mask,
            displaced,
            bank: layout.bank.min(mask.count_ones().saturating_sub(1) as u8),
        })
    }
    fn commit_group(
        &mut self,
        owner: AllocationOwner,
        timbre: u8,
        note: u8,
        cost: u16,
        mask: u32,
        slots: &mut crate::voice_group::VoiceGroupSlots,
    ) {
        for slot in 0..VOICE_COUNT {
            if mask & (1 << slot) != 0 {
                self.claims[slot] = VoiceClaim {
                    owner,
                    note_flags: (note & 127) | 128,
                    release_flags: 0,
                };
                self.budget.costs[slot] = cost;
                slots.timbres[slot] = timbre;
            }
        }
    }

    pub fn finish(&mut self, slot: usize) {
        if let Some(claim) = self.claims.get_mut(slot) {
            claim.note_flags &= 127;
            claim.release_flags &= !3;
            self.budget.costs[slot] = 0;
        }
    }
    /// Original009180/0091B8/00924C retires only live claims belonging to
    /// the edited timbre, promoting each cleared actor in physical order.
    pub fn retire_owner_for_program_edit(&mut self, owner: AllocationOwner) -> u32 {
        let mut retired = 0;
        for slot in 0..VOICE_COUNT {
            let claim = self.claims[slot];
            if claim.owner == owner && (claim.note_flags & 128 != 0 || claim.release_flags & 3 != 0)
            {
                self.finish(slot);
                self.order.promote_released(slot as u8, &self.claims);
                retired |= 1 << slot;
            }
        }
        retired
    }
}
impl ProcessorBudget {
    pub fn remaining(&self) -> [i32; 2] {
        let master = self.costs[..12]
            .iter()
            .fold(self.master_overhead as u32, |sum, &cost| {
                sum.wrapping_add(cost as u32)
            });
        let slave = self.costs[12..]
            .iter()
            .fold(0u32, |sum, &cost| sum.wrapping_add(cost as u32));
        [
            65536u32.wrapping_sub(master) as i32,
            65536u32.wrapping_sub(slave) as i32,
        ]
    }
    pub fn available(&self, cost: i32) -> u8 {
        let remaining = self.remaining();
        ((remaining[0] > cost) as u8) | (((remaining[1] > cost) as u8) << 1)
    }
}

impl VoiceOrder {
    pub fn select(
        &self,
        claims: &[VoiceClaim; VOICE_COUNT],
        owner: AllocationOwner,
        excluded: u32,
        pass: AllocationPass,
    ) -> Option<VoiceSelection> {
        self.select_with_processors(claims, owner, excluded, pass, 3)
    }
    pub fn select_with_processors(
        &self,
        claims: &[VoiceClaim; VOICE_COUNT],
        owner: AllocationOwner,
        excluded: u32,
        pass: AllocationPass,
        processors: u8,
    ) -> Option<VoiceSelection> {
        let mut result = None;
        let mut best = 0;
        for (position, &slot) in self.0.iter().enumerate() {
            if excluded & (1u32 << slot) != 0 {
                continue;
            }
            if matches!(pass, AllocationPass::Fresh) && processors & (1 << (slot / 12)) == 0 {
                continue;
            }
            let priority = claims[slot as usize].priority(owner, position as u8, pass);
            if priority > best {
                best = priority;
                result = Some(VoiceSelection {
                    slot,
                    position: position as u8,
                    priority,
                });
            }
        }
        result
    }
    /// Original 007830: move a selected queue entry to the final position.
    pub fn move_to_back(&mut self, position: u8) {
        let position = position as usize;
        if position >= VOICE_COUNT - 1 {
            return;
        }
        let slot = self.0[position];
        self.0.copy_within(position + 1..VOICE_COUNT, position);
        self.0[VOICE_COUNT - 1] = slot;
    }
    pub fn promote_released(&mut self, slot: u8, claims: &[VoiceClaim; VOICE_COUNT]) {
        let Some(mut position) = self.0.iter().position(|&s| s == slot) else {
            return;
        };
        while position > 0 && claims[self.0[position - 1] as usize].note_flags & 128 != 0 {
            self.0[position] = self.0[position - 1];
            position -= 1;
        }
        self.0[position] = slot;
    }
}
