//! Complete double-buffer FX command producer and service (SYS01D698/01D7E8).
//! Timer and host readiness are inputs; this does not execute FXD03 audio.
use crate::effect_queue::{
    EFFECT_QUEUE_CAPACITY, EffectHostBatch, EffectHostPacket, EffectQueueError,
};
use crate::effect_updates::CoefficientQueueWord;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectRingState {
    pub write_index: u16,
    pub read_index: u16,
    pub count: u16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectTransitionQueueState {
    pub rings: [EffectRingState; 2],
    pub control: u8,
    pub wait_ticks: u16,
    pub wait_started: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectProgramUpload {
    pub destination: u16,
    pub selector: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectTransitionBatch {
    pub coefficients: EffectHostBatch,
    pub coefficient_controls: [u16; 4],
    pub program: Option<EffectProgramUpload>,
}
impl Default for EffectTransitionBatch {
    fn default() -> Self {
        Self {
            coefficients: EffectHostBatch::default(),
            coefficient_controls: [1; 4],
            program: None,
        }
    }
}
/// R4 retained by SYS01D7E8. Scalar SYS01D51C commits (R4 | 1), packed
/// uploads commit 1, and program uploads clear R4 before returning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectQueueHostContext {
    pub scalar_control: u16,
}
pub struct EffectTransitionQueue {
    words: [[CoefficientQueueWord; EFFECT_QUEUE_CAPACITY]; 2],
    state: EffectTransitionQueueState,
}
/// One original producer call suspended at SYS01D742. Its ring pointer is
/// retained across synchronous service, even if service changes control bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingEffectQueueWord {
    ring: usize,
    word: CoefficientQueueWord,
    pending_switch: bool,
}

/// Owns a sequential producer cursor independently of its command storage.
/// Pending calls retain the original ring and host context across service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectQueuePublicationCursor {
    cursor: usize,
    pending: Option<PendingEffectQueueWord>,
    last_pending_switch: bool,
    host_context: EffectQueueHostContext,
    address_wrapper: bool,
}
impl Default for EffectQueuePublicationCursor {
    fn default() -> Self {
        Self {
            address_wrapper: true,
            ..Self::with_host_control(0)
        }
    }
}
impl EffectQueuePublicationCursor {
    pub fn with_host_control(scalar_control: u16) -> Self {
        Self {
            cursor: 0,
            pending: None,
            last_pending_switch: false,
            host_context: EffectQueueHostContext { scalar_control },
            address_wrapper: false,
        }
    }
    pub fn host_context(&mut self) -> &mut EffectQueueHostContext {
        &mut self.host_context
    }
    pub fn published_words(&self) -> usize {
        self.cursor
    }
    pub fn pending_switch(&self) -> bool {
        self.last_pending_switch
    }
    /// Publishes exactly one caller word. A suspended call retains its word
    /// and ring; resuming it must not prepare another producer call.
    pub fn publish_word(
        &mut self,
        queue: &mut EffectTransitionQueue,
        word: CoefficientQueueWord,
    ) -> bool {
        if self.pending.is_none() && self.address_wrapper {
            self.host_context.scalar_control = word.address;
        }
        let pending = *self.pending.get_or_insert_with(|| queue.begin_word(word));
        let Some(pending_switch) = queue.try_publish_word(pending) else {
            return false;
        };
        self.last_pending_switch = pending_switch;
        self.pending = None;
        self.cursor += 1;
        true
    }
}
/// Sequential SYS01D698 publication. Unlike atomic enqueue_words, this can
/// service a full ring between individual words of a packed command.
pub struct EffectQueuePublication<'a> {
    words: &'a [CoefficientQueueWord],
    cursor: EffectQueuePublicationCursor,
}
impl<'a> EffectQueuePublication<'a> {
    /// Reproduces the SYS005E50 address/value wrapper's R4 for each word.
    pub fn new(words: &'a [CoefficientQueueWord]) -> Self {
        Self {
            words,
            cursor: EffectQueuePublicationCursor::default(),
        }
    }
    /// Bare SYS01D698 with its caller's R4.
    pub fn with_host_control(words: &'a [CoefficientQueueWord], scalar_control: u16) -> Self {
        Self {
            words,
            cursor: EffectQueuePublicationCursor::with_host_control(scalar_control),
        }
    }
    pub fn host_context(&mut self) -> &mut EffectQueueHostContext {
        self.cursor.host_context()
    }
    pub fn published_words(&self) -> usize {
        self.cursor.published_words()
    }
    pub fn pending_switch(&self) -> bool {
        self.cursor.pending_switch()
    }
    /// Returns true once every word has been accepted, or false when the
    /// pinned producer ring needs an original queue-service step.
    pub fn publish_available(&mut self, queue: &mut EffectTransitionQueue) -> bool {
        while let Some(&word) = self.words.get(self.cursor.published_words()) {
            if !self.cursor.publish_word(queue, word) {
                return false;
            }
        }
        true
    }
}
impl Default for EffectTransitionQueue {
    fn default() -> Self {
        Self::with_control(0, 0, 0)
    }
}
impl EffectTransitionQueue {
    pub fn with_control(control: u8, wait_ticks: u16, wait_started: u16) -> Self {
        Self {
            words: [[CoefficientQueueWord::default(); EFFECT_QUEUE_CAPACITY]; 2],
            state: EffectTransitionQueueState {
                control,
                wait_ticks,
                wait_started,
                ..EffectTransitionQueueState::default()
            },
        }
    }
    pub fn state(&self) -> EffectTransitionQueueState {
        self.state
    }
    /// Declared producer/consumer ring state for restoration and independent
    /// reference fixtures. Unused words remain intact across ring transitions.
    pub fn from_state(
        state: EffectTransitionQueueState,
        words: [[CoefficientQueueWord; EFFECT_QUEUE_CAPACITY]; 2],
    ) -> Result<Self, EffectQueueError> {
        if state
            .rings
            .iter()
            .any(|r| r.write_index >= 2048 || r.read_index >= 2048 || r.count > 2046)
        {
            return Err(EffectQueueError::Full);
        }
        Ok(Self { words, state })
    }
    pub fn ring_words(
        &self,
        ring: usize,
    ) -> Option<&[CoefficientQueueWord; EFFECT_QUEUE_CAPACITY]> {
        self.words.get(ring)
    }

    fn producer(state: &mut EffectTransitionQueueState, tag: u8) -> (usize, bool) {
        let pending = tag == 3 && (state.control & 1 != (state.control >> 1) & 1);
        if tag == 3 && !pending {
            state.control ^= 1;
        }
        let ring = usize::from(state.control & 1);
        if tag == 3 {
            state.rings[ring] = EffectRingState::default();
        }
        (ring, pending)
    }
    fn begin_word(&mut self, word: CoefficientQueueWord) -> PendingEffectQueueWord {
        let (ring, pending_switch) =
            Self::producer(&mut self.state, (word.tagged_value >> 24) as u8);
        PendingEffectQueueWord {
            ring,
            word,
            pending_switch,
        }
    }
    fn try_publish_word(&mut self, pending: PendingEffectQueueWord) -> Option<bool> {
        let state = &mut self.state.rings[pending.ring];
        if state.count >= 2046 {
            return None;
        }
        self.words[pending.ring][usize::from(state.write_index)] = pending.word;
        state.write_index = (state.write_index + 1) & 2047;
        state.count += 1;
        Some(pending.pending_switch)
    }
    /// Accept a whole publication atomically. The source producer services a
    /// full ring synchronously; this bounded adapter reports backpressure.
    /// Tag 3 resets the producer ring, including when a switch is pending.
    pub fn enqueue_words(
        &mut self,
        words: &[CoefficientQueueWord],
    ) -> Result<bool, EffectQueueError> {
        let mut cursor = 0;
        while cursor < words.len() {
            let tag = words[cursor].tagged_value >> 24;
            let count = if tag & 0x80 != 0 {
                (tag & 7).max(1) as usize
            } else {
                1
            };
            if cursor + count > words.len() {
                return Err(EffectQueueError::IncompletePacket);
            }
            cursor += count;
        }
        let mut next = self.state;
        let mut pending = false;
        for word in words {
            let (ring, p) = Self::producer(&mut next, (word.tagged_value >> 24) as u8);
            pending = p;
            if next.rings[ring].count >= 2046 {
                return Err(EffectQueueError::Full);
            }
            next.rings[ring].write_index = (next.rings[ring].write_index + 1) & 2047;
            next.rings[ring].count += 1;
        }
        for &word in words {
            let (ring, _) = Self::producer(&mut self.state, (word.tagged_value >> 24) as u8);
            let s = &mut self.state.rings[ring];
            self.words[ring][usize::from(s.write_index)] = word;
            s.write_index = (s.write_index + 1) & 2047;
            s.count += 1;
        }
        Ok(pending)
    }
    fn pop(&mut self, ring: usize) -> CoefficientQueueWord {
        let s = &mut self.state.rings[ring];
        let word = self.words[ring][usize::from(s.read_index)];
        s.read_index = (s.read_index + 1) & 2047;
        s.count -= 1;
        word
    }
    pub fn service(&mut self, tick: u16, host_status: u16) -> EffectTransitionBatch {
        self.service_with_context(tick, host_status, &mut EffectQueueHostContext::default())
    }
    pub fn service_with_context(
        &mut self,
        tick: u16,
        host_status: u16,
        context: &mut EffectQueueHostContext,
    ) -> EffectTransitionBatch {
        let mut output = EffectTransitionBatch::default();
        if self.state.control & 4 == 0 && self.state.control & 1 != (self.state.control >> 1) & 1 {
            self.state.control ^= 2;
            self.state.wait_ticks = 0;
        } else if self.state.wait_ticks != 0 {
            // MOV.W sign extends the wait, whereas elapsed is EXTU.W before CMP/HS.
            let wait = self.state.wait_ticks as i16 as i32 as u32;
            if u32::from(tick.wrapping_sub(self.state.wait_started)) < wait {
                return output;
            }
            self.state.wait_ticks = 0;
        }
        if host_status & 3 != 0 {
            return output;
        }
        let ring = usize::from((self.state.control >> 1) & 1);
        for _ in 0..4 {
            if self.state.rings[ring].count == 0 {
                break;
            }
            let word = self.pop(ring);
            let tag = (word.tagged_value >> 24) as u8;
            match tag {
                1 => {
                    self.state.wait_ticks = word.tagged_value as u16;
                    self.state.wait_started = tick;
                    break;
                }
                2 => {
                    output.program = Some(EffectProgramUpload {
                        destination: word.address,
                        selector: word.tagged_value as u8 & 127,
                    });
                    context.scalar_control = 0;
                    break;
                }
                3 => {
                    self.state.control |= 4;
                    break;
                }
                4 => {
                    self.state.control &= !4;
                    break;
                }
                5..=127 => continue,
                _ => {
                    let count = if tag & 128 != 0 { (tag & 7).max(1) } else { 1 };
                    let mut packet = EffectHostPacket {
                        address: word.address,
                        values: [0; 7],
                        count: 1,
                    };
                    packet.values[0] = word.tagged_value & 0xffffff;
                    while packet.count < count && self.state.rings[ring].count != 0 {
                        packet.values[usize::from(packet.count)] =
                            self.pop(ring).tagged_value & 0xffffff;
                        packet.count += 1;
                    }
                    output.coefficients.packets[usize::from(output.coefficients.count)] = packet;
                    output.coefficient_controls[usize::from(output.coefficients.count)] =
                        if tag & 128 != 0 {
                            1
                        } else {
                            context.scalar_control | 1
                        };
                    output.coefficients.count += 1;
                    if tag & 128 != 0 {
                        break;
                    }
                }
            }
        }
        output
    }
}

/// Original program-bank selection, before reading mutable buffer contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectProgramBufferLocation {
    pub word_address: u32,
    pub count_address: u32,
}
#[derive(Clone, Copy)]
pub struct EffectProgramBufferLayout {
    pub normal: u32,
    pub selected_insert: u32,
    pub selected_master: u32,
}
impl EffectProgramBufferLayout {
    /// SYS0745D0 and SYS074720. Selectors 87..127 have no defined buffer;
    /// the source falls through with an uninitialised stack pointer there.
    pub fn locate(self, selector: u8) -> Option<EffectProgramBufferLocation> {
        let (base, stride, offset, count_offset) = match selector {
            0..=59 => (
                self.normal,
                0x44a,
                [0, 6, 0x43e][usize::from(selector % 3)],
                0x444,
            ),
            60..=83 => (
                self.selected_insert,
                0x22e,
                [0, 6, 0x222][usize::from(selector % 3)],
                0x228,
            ),
            84..=86 => (
                self.selected_master,
                0,
                [0, 6, 0x2d6][usize::from(selector % 3)],
                0x2dc,
            ),
            _ => return None,
        };
        let slot = if selector < 60 {
            selector / 3
        } else if selector < 84 {
            (selector - 60) / 3
        } else {
            0
        };
        Some(EffectProgramBufferLocation {
            word_address: base
                .wrapping_add(u32::from(slot) * stride)
                .wrapping_add(offset),
            count_address: base
                .wrapping_add(u32::from(slot) * stride)
                .wrapping_add(count_offset + u32::from(selector % 3) * 2),
        })
    }
}
