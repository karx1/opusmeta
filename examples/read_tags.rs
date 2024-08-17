use opusmeta::{read_from, Result};
use std::fs::File;

fn main() -> Result<()> {
    let file = File::open("dysti.opus").unwrap();
    read_from(file)?;
    Ok(())
}
