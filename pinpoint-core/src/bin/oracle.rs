use pinpoint_core::{presentation, source, transition};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct TransitionProbe {
    durations: [u32; 3],
    samples: [transition::TransitionState; 5],
}

fn print_json(value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments
        .next()
        .ok_or("usage: pinpoint-rust-oracle MODE FILE")?;
    let filename = arguments
        .next()
        .ok_or("usage: pinpoint-rust-oracle MODE FILE")?;
    if arguments.next().is_some() {
        return Err("usage: pinpoint-rust-oracle MODE FILE".into());
    }
    match mode.as_str() {
        "presentation" => print_json(&presentation::load(Path::new(&filename), false)?),
        "presentation-ignore-comments" => {
            print_json(&presentation::load(Path::new(&filename), true)?)
        }
        "source" => {
            let bytes = std::fs::read(filename)?;
            let source_text = std::str::from_utf8(&bytes)?;
            print_json(&source::analyze(source_text))
        }
        "transition" => {
            let path = Path::new(&filename);
            let presentation = presentation::load(path, false)?;
            let name = &presentation
                .slides
                .first()
                .ok_or("presentation has no slides")?
                .transition;
            let json_name = if name.ends_with(".json") {
                name.clone()
            } else {
                format!("{name}.json")
            };
            let transition = transition::LegacyTransition::load(
                &path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(json_name),
            )?;
            print_json(&TransitionProbe {
                durations: [
                    transition.duration(true, false),
                    transition.duration(false, false),
                    transition.duration(false, true),
                ],
                samples: [
                    transition.calculate(true, false, 0.0),
                    transition.calculate(true, false, 0.5),
                    transition.calculate(false, false, 1.0),
                    transition.calculate(true, true, 1.0),
                    transition.calculate(false, true, 1.0),
                ],
            })
        }
        _ => Err(format!("unknown oracle mode: {mode}").into()),
    }
}
