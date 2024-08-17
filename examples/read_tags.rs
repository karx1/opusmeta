use opusmeta::{read_from_path, replace, Result};
use std::fs::OpenOptions;

fn main() -> Result<()> {
    let path = std::env::args_os().nth(1).expect("No input file specified");
    let mut comments = read_from_path(&path)?;
    println!("{comments:#?}");

    comments.add_one("ARTIST".into(), "Someone Else".into());
    println!("{comments:#?}");

    let file = OpenOptions::new().read(true).write(true).open(path)?;
    replace(file, comments).unwrap();
    Ok(())
}
