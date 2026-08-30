use serde_json::Value;
use std::process::Command;

fn read_json(command: &mut Command) -> Result<Value, Box<dyn std::error::Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "oracle failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let c_oracle = arguments
        .next()
        .ok_or("usage: pinpoint-compare-oracles C_ORACLE FIXTURE...")?;
    let rust_oracle = std::env::current_exe()?
        .parent()
        .ok_or("Rust oracle executable has no parent")?
        .join("pinpoint-rust-oracle");
    let fixtures: Vec<String> = arguments.collect();
    if fixtures.is_empty() {
        return Err("usage: pinpoint-compare-oracles C_ORACLE FIXTURE...".into());
    }

    let mut failures = 0;
    for fixture in fixtures {
        let mut modes = vec!["presentation", "presentation-ignore-comments", "source"];
        if fixture.ends_with("legacy-transition.pin") {
            modes.push("transition");
        }
        for mode in modes {
            let c_value = read_json(Command::new(&c_oracle).arg(mode).arg(&fixture))?;
            let rust_value = read_json(Command::new(&rust_oracle).arg(mode).arg(&fixture))?;
            if c_value == rust_value {
                println!("PASS {mode} {fixture}");
            } else {
                failures += 1;
                eprintln!("FAIL {mode} {fixture}");
                eprintln!("  C:    {}", serde_json::to_string_pretty(&c_value)?);
                eprintln!("  Rust: {}", serde_json::to_string_pretty(&rust_value)?);
            }
        }
    }
    if failures > 0 {
        Err(format!("{failures} differential comparison(s) failed").into())
    } else {
        Ok(())
    }
}
