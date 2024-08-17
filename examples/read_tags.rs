use opusmeta::{read_from_path, Result};

fn main() -> Result<()> {
    let path = std::env::args_os().nth(1).expect("No input file specified");
    let comments = read_from_path(path)?;
    println!("{comments:#?}");
    Ok(())
}
