//! Replays test-only checkpoints produced by the separate unchanged C++ suite.
use radias_domain::dsp::c55::C55;
use std::{
    fs,
    io::{BufReader, Read},
    path::PathBuf,
};

fn blob(stream: &mut impl Read) -> Result<Option<Vec<u8>>, String> {
    let mut size = [0u8; 4];
    let first = stream.read(&mut size[..1]).map_err(|e| e.to_string())?;
    if first == 0 {
        return Ok(None);
    }
    stream
        .read_exact(&mut size[1..])
        .map_err(|e| e.to_string())?;
    let size = u32::from_le_bytes(size) as usize;
    if size > 32 * 1024 * 1024 {
        return Err("oversized oracle record".into());
    }
    let mut value = vec![0; size];
    stream.read_exact(&mut value).map_err(|e| e.to_string())?;
    Ok(Some(value))
}
fn required(stream: &mut impl Read) -> Result<Vec<u8>, String> {
    blob(stream)?.ok_or("truncated oracle record".into())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    let path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "runs/rust-c55/oracle.bin".into());
    let mut stream = BufReader::new(fs::File::open(&path).map_err(|e| e.to_string())?);
    let output = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| "runs/rust-c55".into());
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    // Expected decoder failures are captured in the report, not printed by the runtime hook.
    std::panic::set_hook(Box::new(|_| {}));
    let mut passed = 0;
    let mut failed = Vec::new();
    let start = std::time::Instant::now();
    while let Some(header) = blob(&mut stream)? {
        if header.len() != 9 {
            return Err("invalid oracle operation".into());
        }
        let id = u32::from_le_bytes(header[..4].try_into().unwrap());
        let mode = header[4];
        let count = u32::from_le_bytes(header[5..9].try_into().unwrap());
        let before = required(&mut stream)?;
        let expected = required(&mut stream)?;
        let exception = required(&mut stream)?;
        if exception.len() < 4 {
            return Err("invalid exception blob".into());
        }
        let exception_len = u32::from_le_bytes(exception[..4].try_into().unwrap()) as usize;
        let expected_exception = std::str::from_utf8(
            exception
                .get(4..4 + exception_len)
                .ok_or("invalid exception blob")?,
        )
        .map_err(|e| e.to_string())?;
        let mut cpu = C55::from_checkpoint_bytes(&before)?;
        if cpu.checkpoint_bytes() != before {
            return Err(format!("input checkpoint {id} did not roundtrip"));
        }
        if exception.len() > 4 + exception_len {
            let at = 4 + exception_len;
            let ordinal = u32::from_le_bytes(
                exception
                    .get(at..at + 4)
                    .ok_or("invalid observer fixture")?
                    .try_into()
                    .unwrap(),
            );
            let length = u32::from_le_bytes(
                exception
                    .get(at + 4..at + 8)
                    .ok_or("invalid observer fixture")?
                    .try_into()
                    .unwrap(),
            ) as usize;
            let message = std::str::from_utf8(
                exception
                    .get(at + 8..at + 8 + length)
                    .ok_or("invalid observer fixture")?,
            )
            .map_err(|e| e.to_string())?
            .to_string();
            if ordinal != 0 {
                let mut observed = 0;
                cpu.store_observer = Some(Box::new(move |_, _, _, _| {
                    observed += 1;
                    if observed == ordinal {
                        std::panic::panic_any(message.clone());
                    }
                }));
            }
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match mode {
            1 => cpu.run(count),
            2 => cpu.step(),
            _ => panic!("invalid oracle operation"),
        }));
        let actual_exception = result
            .err()
            .map(|e| {
                e.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|x| x.to_string()))
                    .unwrap_or_else(|| "unknown panic".into())
            })
            .unwrap_or_default();
        let actual = cpu.checkpoint_bytes();
        let exact = actual == expected && actual_exception == expected_exception;
        if exact {
            passed += 1;
        } else {
            let difference = actual
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(actual.len().min(expected.len()));
            if failed.len() < 12 {
                fs::write(output.join(format!("{id}-actual.bin")), &actual)
                    .map_err(|e| e.to_string())?;
                fs::write(output.join(format!("{id}-expected.bin")), &expected)
                    .map_err(|e| e.to_string())?;
                fs::write(output.join(format!("{id}-before.bin")), &before)
                    .map_err(|e| e.to_string())?;
            }
            failed.push(format!("{{\"case\":{id},\"mode\":{mode},\"count\":{count},\"first_differing_checkpoint_byte\":{difference},\"actual_bytes\":{},\"expected_bytes\":{},\"exception_equal\":{}}}",actual.len(),expected.len(),actual_exception==expected_exception));
        }
    }
    let report = format!(
        "{{\"scope\":\"C55 operations from separate C++ test suite; hardware and full signal-path fidelity unverified\",\"passed\":{passed},\"failed\":[{}],\"elapsed_seconds\":{},\"exact_match\":{}}}\n",
        failed.join(","),
        start.elapsed().as_secs_f64(),
        failed.is_empty()
    );
    fs::write(output.join("parity.json"), &report).map_err(|e| e.to_string())?;
    println!(
        "C55 checkpoint parity: {passed} passed, {} failed ({:.2}s)",
        failed.len(),
        start.elapsed().as_secs_f64()
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err("C55 state parity failed; see parity.json".into())
    }
}
