use rich_record::{record, render::raster::Fonts, tape};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: intentumdiff-record-demo TAPE OUTPUT_DIR BIN_DIR".into());
    }
    let input = PathBuf::from(&args[0]);
    let tape = tape::parse(&std::fs::read_to_string(&input)?)?;
    let options = record::Options {
        bin_dir: Some(PathBuf::from(&args[2]).canonicalize()?),
        repo: Some(std::env::current_dir()?),
        ..Default::default()
    };
    let recording = record::record(&tape, "demo", &options)?;
    record::write(
        &recording,
        &PathBuf::from(&args[1]),
        "demo",
        record::Formats::ALL,
        &Fonts::embedded(),
        &options.theme,
        None,
    )?;
    Ok(())
}
