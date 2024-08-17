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
    #[error("Encountered a comment which was formatted wrong or was not valid UTF-8.")]
    MalformedComment(Vec<u8>),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Tag {
    comments: Vec<(String, String)>,
}

impl Tag {
    pub fn new(comments: Vec<(String, String)>) -> Self {
        Self { comments }
    }
}

pub fn read_from<R: Read + Seek>(f_in: R) -> Result<Tag> {
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
    let vendor_length = u32::from_le_bytes(buffer);
    // we don't care about what the vendor actually is so we can skip it
    cursor.seek_relative(vendor_length.into())?;
    let mut buffer = [0; 4];
    cursor.read_exact(&mut buffer)?;
    let comment_count = u32::from_le_bytes(buffer);
    let mut comments: Vec<(String, String)> = Vec::new();
    for _ in 0..comment_count {
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        // only panics on platforms where usize < 32 bits
        let comment_length: usize = u32::from_le_bytes(buffer).try_into().unwrap();
        let mut buffer = vec![0; comment_length];
        cursor.read_exact(&mut buffer)?;
        let comment =
            std::str::from_utf8(&buffer).map_err(|_| Error::MalformedComment(buffer.clone()))?;
        let pair = comment
            .split_once('=')
            .map(|(tag, value)| (tag.to_string(), value.to_string()))
            .ok_or_else(|| Error::MalformedComment(buffer.clone()))?;
        comments.push(pair);
    }
    Ok(Tag::new(comments))
}
