use ogg::PacketReader;
use std::io::Cursor;
use std::io::{Read, Seek};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    ReadError(#[from] ogg::OggReadError),
    #[error("The selected file is not an opus file")]
    NotOpus,
    #[error("Expected a packet but did not receive one")]
    MissingPacket,
    #[error("The comment header was malformed: {0}")]
    HeaderError(#[from] std::io::Error),
    #[error("The error was malformed: {0}")]
    MalformedPacket(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn read_from<R: Read + Seek>(f_in: R) -> Result<()> {
    let mut reader = PacketReader::new(f_in);
    let first_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
    if !first_packet.data.starts_with("OpusHead".as_bytes()) {
        return Err(Error::NotOpus);
    }
    let header_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
    println!("{}", String::from_utf8_lossy(&header_packet.data));
    let mut cursor = Cursor::new(header_packet.data);
    cursor.seek_relative(8)?; // length of string "OpusTags"
    let mut buffer = [0; 4];
    cursor.read_exact(&mut buffer)?;
    // only panics on platforms where usize < 32 bits
    let vendor_length = u32::from_le_bytes(buffer);
    // we don't care about what the vendor actually is so we can skip it
    cursor.seek_relative(vendor_length.into())?;
    Ok(())
}
