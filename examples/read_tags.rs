use opusmeta::{Result, Tag};

fn main() -> Result<()> {
    let path = std::env::args_os().nth(1).expect("No input file specified");
    let mut comments = Tag::read_from_path(&path)?;
    println!("{comments:#?}");

    comments.add_one("ARTIST".into(), "Someone Else".into());
    println!("{comments:#?}");

    comments.write_to_path(path).unwrap();
    Ok(())
}
