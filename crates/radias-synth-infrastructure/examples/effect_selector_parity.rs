use radias_synth_application::effects::{EffectProgramPort, set_effect_selector};
use radias_synth_domain::effect_setters::EffectSelectorWrites;
use serde_json::json;
use std::{collections::BTreeSet, fs, path::PathBuf};
#[derive(Default)]
struct Port(Vec<(u16, u32, u16)>);
impl EffectProgramPort for Port {
    type Error = std::convert::Infallible;
    fn upload_program(&mut self, _: u16, _: &[u64], _: u16) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn write_coefficient(
        &mut self,
        address: u16,
        value: u32,
        control: u16,
    ) -> Result<(), Self::Error> {
        self.0.push((address, value, control));
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-selector-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated source selector words".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x45465331 {
        return Err("Unsupported source selector format".into());
    }
    let (mut cursor, mut cases, mut errors, mut host_words) = (1usize, 0usize, 0usize, 0usize);
    let mut seen = BTreeSet::new();
    let mut first = serde_json::Value::Null;
    while cursor < words.len() {
        let [origin, offset, selector, count]: [u32; 4] =
            words[cursor..cursor + 4].try_into().unwrap();
        cursor += 4;
        let candidate = EffectSelectorWrites::compile(origin as u16, offset, selector);
        let mut difference = u32::from(candidate.count) != count;
        for i in 0..count as usize {
            difference |= u32::from(candidate.words[i].address) != words[cursor]
                || candidate.words[i].tagged_value != words[cursor + 1];
            cursor += 2;
        }
        let packets = words[cursor];
        let original_words = words[cursor + 1];
        cursor += 2;
        let mut port = Port::default();
        set_effect_selector(&mut port, &candidate)?;
        port.0.sort_unstable();
        difference |= port.0.len() != original_words as usize || port.0.len() != packets as usize;
        for value in &port.0 {
            difference |=
                [u32::from(value.0), value.1, u32::from(value.2)] != words[cursor..cursor + 3];
            cursor += 3;
        }
        if difference {
            errors += 1;
            if first.is_null() {
                first = json!({"origin":origin,"offset":offset,"selector":selector});
            }
        }
        host_words += port.0.len();
        cases += 1;
        seen.insert((origin, offset, selector));
    }
    let mut expected = BTreeSet::new();
    for (origin, offset) in [(0, 0), (17, 19), (0xfffe, 0xffff), (0xffff, 0x8001fedc)] {
        for selector in (0..256).chain(std::iter::once(u32::MAX)) {
            expected.insert((origin, offset, selector));
        }
    }
    let passed = errors == 0 && seen == expected && cases == 1028 && host_words == 36;
    fs::write(
        root.join("runs/native-clone/effect-selector-parity.json"),
        serde_json::to_string_pretty(
            &json!({"passed":passed,"whole_original_selector_calls":cases,"errors":errors,"host_words_compared":host_words,"first_difference":first,"original_selector_outputs_used_as_native_inputs":false,"FXD03_audio_arithmetic_verified":false}),
        )? + "\n",
    )?;
    println!("Native effect selector: {cases} original calls, {errors} differences");
    if !passed {
        return Err("Native effect selector differs".into());
    }
    Ok(())
}
