use std::collections::BTreeMap;
pub struct RegisterBytes {
    values: Box<[u8; 0x1300]>,
    present: Box<[bool; 0x1300]>,
    other: BTreeMap<u32, u8>,
}
impl Default for RegisterBytes {
    fn default() -> Self {
        Self {
            values: Box::new([0; 0x1300]),
            present: Box::new([false; 0x1300]),
            other: BTreeMap::new(),
        }
    }
}
impl RegisterBytes {
    fn index(a: u32) -> usize {
        let x = a.wrapping_sub(0xfffff000);
        if x < 0x1000 {
            return x as usize;
        }
        let x = a.wrapping_sub(0xa4000000);
        if x < 0x200 {
            return 0x1000 + x as usize;
        }
        let x = a.wrapping_sub(0xa8000000);
        if x < 0x100 {
            return 0x1200 + x as usize;
        }
        0x1300
    }
    pub fn clear(&mut self) {
        self.values.fill(0);
        self.present.fill(false);
        self.other.clear();
    }
    pub fn contains(&self, a: u32) -> bool {
        let i = Self::index(a);
        if i < 0x1300 {
            self.present[i]
        } else {
            self.other.contains_key(&a)
        }
    }
    pub fn read(&self, a: u32) -> u8 {
        let i = Self::index(a);
        if i < 0x1300 {
            self.values[i]
        } else {
            self.other.get(&a).copied().unwrap_or(0)
        }
    }
    pub fn get(&mut self, a: u32) -> u8 {
        let i = Self::index(a);
        if i < 0x1300 {
            self.present[i] = true;
            self.values[i]
        } else {
            *self.other.entry(a).or_default()
        }
    }
    pub fn set(&mut self, a: u32, v: u8) {
        let i = Self::index(a);
        if i < 0x1300 {
            self.present[i] = true;
            self.values[i] = v;
        } else {
            self.other.insert(a, v);
        }
    }
    pub fn read16(&self, a: u32) -> u16 {
        (self.read(a) as u16) << 8 | self.read(a.wrapping_add(1)) as u16
    }
    pub fn read32(&self, a: u32) -> u32 {
        let mut v = 0;
        for i in 0..4 {
            v = v << 8 | self.read(a.wrapping_add(i)) as u32;
        }
        v
    }
    pub fn write16(&mut self, a: u32, v: u16) {
        self.set(a, (v >> 8) as u8);
        self.set(a.wrapping_add(1), v as u8);
    }
    pub fn write32(&mut self, a: u32, v: u32) {
        for i in 0..4 {
            self.set(a.wrapping_add(i), (v >> (24 - 8 * i)) as u8);
        }
    }
}
