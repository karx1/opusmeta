use opusmeta::{read_from_path, write_to_path, Result};

fn main() -> Result<()> {
    let path = std::env::args_os().nth(1).expect("No input file specified");
    let mut comments = read_from_path(&path)?;
    println!("{comments:#?}");

    comments.add_one("ARTIST".into(), "Someone Else".into());
    println!("{comments:#?}");

    write_to_path(path, comments).unwrap();
    Ok(())
}
