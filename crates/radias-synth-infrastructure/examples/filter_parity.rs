use radias_synth_domain::filter::ResonantFilter;
use radias_synth_infrastructure::{oracle::decode_filter_records, wav};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().map(String::as_str).unwrap_or(".."));
    let output = root.join("runs/native-clone");
    let mut sets = vec![(
        "isolated".to_owned(),
        output.join("original-filters.bin"),
        false,
    )];
    for prefix in args.iter().skip(1) {
        sets.push((
            prefix.clone(),
            output.join(format!("{prefix}-filter-stream.bin")),
            true,
        ));
    }
    let mut reports = Vec::new();
    let mut failed = false;
    for (name, path, continuous) in sets {
        let records = decode_filter_records(&fs::read(path)?)?;
        let mut filter = ResonantFilter {
            state: records[0].initial,
        };
        let mut samples = Vec::with_capacity(records.len());
        let mut mismatches = 0;
        for (index, record) in records.iter().enumerate() {
            if !continuous {
                filter.state = record.initial;
            }
            if filter.state != record.initial {
                return Err(
                    format!("Original filter state discontinuity at {name}/{index}").into(),
                );
            }
            let actual = filter.next_sample(record.input, record.coefficients);
            if actual != record.output || filter.state != record.final_state {
                if mismatches < 3 {
                    eprintln!(
                        "{name}/{index}: output {} != {}, state {:?} != {:?}",
                        actual.0, record.output.0, filter.state, record.final_state
                    );
                }
                mismatches += 1;
            }
            samples.push(actual);
        }
        let nonzero = samples.iter().filter(|s| s.0 != 0).count();
        if nonzero == 0 {
            return Err("Filter oracle is silent".into());
        }
        if continuous {
            wav::write_mono(&output.join(format!("{name}-rust-filter.wav")), &samples)?;
        }
        reports.push(serde_json::json!({"name":name,"frames":samples.len(),"mismatches":mismatches,"nonzero":nonzero,"continuous_state":continuous}));
        failed |= mismatches != 0;
    }
    let report = serde_json::json!({"scope":"Original B4C0..B57F resonant filter samples and full state","sets":reports,"passed":!failed});
    fs::write(
        output.join("filter-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if failed {
        return Err("Filter parity failed".into());
    }
    Ok(())
}
