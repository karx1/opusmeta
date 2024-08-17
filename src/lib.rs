use ogg::PacketReader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Cursor;
use std::io::{Read, Seek};
use std::path::Path;
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

#[derive(Debug)]
pub struct Tag {
    comments: HashMap<String, Vec<String>>,
}

impl Tag {
    pub fn new(comments: Vec<(String, String)>) -> Self {
        let mut comments_map = HashMap::new();
        for (key, value) in comments.into_iter() {
            comments_map
                .entry(key)
                .and_modify(|v: &mut Vec<String>| v.push(value.clone()))
                .or_insert(vec![value]);
        }

        Self {
            comments: comments_map,
        }
    }

    pub fn add_one(&mut self, tag: String, value: String) {
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.push(value.clone()))
            .or_insert(vec![value]);
    }

    pub fn add_many(&mut self, tag: String, mut values: Vec<String>) {
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.append(&mut values))
            .or_insert(values);
    }

    pub fn get(&self, tag: &str) -> Option<&Vec<String>> {
        self.comments.get(tag)
    }

    pub fn remove_entries(&mut self, tag: &str) -> Option<Vec<String>> {
        self.comments.remove(tag)
    }
}

pub fn read_from<R: Read + Seek>(f_in: R) -> Result<Tag> {
    let mut reader = PacketReader::new(f_in);
    let first_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
    if !first_packet.data.starts_with("OpusHead".as_bytes()) {
        return Err(Error::NotOpus);
    }
    let header_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
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

pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<Tag> {
    let file = File::open(path)?;
    read_from(file)
}
