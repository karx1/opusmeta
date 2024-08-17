use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::{Read, Seek, Write};
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
    #[error("The packet was malformed")]
    MalformedPacket,
    #[error("Encountered a comment which was formatted wrong or was not valid UTF-8.")]
    MalformedComment(Vec<u8>),
    #[error("Expected UTF-8 content, but it was invalid")]
    UTFError(#[from] std::string::FromUtf8Error),
    #[error("The content was too big for the Opus spec")]
    TooBigError(#[from] std::num::TryFromIntError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Tag {
    vendor: String,
    comments: HashMap<String, Vec<String>>,
}

impl Tag {
    pub fn new(vendor: String, comments: Vec<(String, String)>) -> Self {
        let mut comments_map = HashMap::new();
        for (mut key, value) in comments.into_iter() {
            key.make_ascii_lowercase();
            comments_map
                .entry(key)
                .and_modify(|v: &mut Vec<String>| v.push(value.clone()))
                .or_insert(vec![value]);
        }

        Self {
            vendor,
            comments: comments_map,
        }
    }

    pub fn add_one(&mut self, mut tag: String, value: String) {
        tag.make_ascii_lowercase();
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.push(value.clone()))
            .or_insert(vec![value]);
    }

    pub fn add_many(&mut self, mut tag: String, mut values: Vec<String>) {
        tag.make_ascii_lowercase();
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.append(&mut values))
            .or_insert(values);
    }

    pub fn get(&self, mut tag: String) -> Option<&Vec<String>> {
        tag.make_ascii_lowercase();
        self.comments.get(&tag)
    }

    pub fn remove_entries(&mut self, mut tag: String) -> Option<Vec<String>> {
        tag.make_ascii_lowercase();
        self.comments.remove(&tag)
    }

    pub fn get_vendor(&self) -> &str {
        &self.vendor
    }

    pub fn set_vendor(&mut self, new_vendor: String) {
        self.vendor = new_vendor;
    }
}

impl Tag {
    pub fn read_from<R: Read + Seek>(f_in: R) -> Result<Self> {
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
        // only panics on platforms where usize < 32 bits
        let vendor_length: usize = u32::from_le_bytes(buffer).try_into()?;
        let mut buffer = vec![0; vendor_length];
        cursor.read_exact(&mut buffer)?;
        let vendor = String::from_utf8(buffer)?;
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let comment_count = u32::from_le_bytes(buffer);
        let mut comments: Vec<(String, String)> = Vec::new();
        for _ in 0..comment_count {
            let mut buffer = [0; 4];
            cursor.read_exact(&mut buffer)?;
            // only panics on platforms where usize < 32 bits
            let comment_length: usize = u32::from_le_bytes(buffer).try_into()?;
            let mut buffer = vec![0; comment_length];
            cursor.read_exact(&mut buffer)?;
            let comment = std::str::from_utf8(&buffer)
                .map_err(|_| Error::MalformedComment(buffer.clone()))?;
            let pair = comment
                .split_once('=')
                .map(|(tag, value)| (tag.to_string(), value.to_string()))
                .ok_or_else(|| Error::MalformedComment(buffer.clone()))?;
            comments.push(pair);
        }
        Ok(Self::new(vendor, comments))
    }

    pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        Self::read_from(file)
    }

    pub fn write_to<W: Read + Write + Seek>(&self, mut f_in: W) -> Result<()> {
        let f_out_raw: Vec<u8> = vec![];
        let mut cursor = Cursor::new(f_out_raw);

        let mut reader = PacketReader::new(&mut f_in);
        let mut writer = PacketWriter::new(&mut cursor);

        // first packet
        {
            let first_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
            writer.write_packet(
                first_packet.data.clone(),
                first_packet.stream_serial(),
                get_end_info(&first_packet),
                first_packet.absgp_page(),
            )?;
        }

        // second packet, which is the comment header
        {
            let comment_header_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
            let new_pack_data = self.construct_packet_from_tag()?;
            writer.write_packet(
                new_pack_data,
                comment_header_packet.stream_serial(),
                PacketWriteEndInfo::EndPage,
                comment_header_packet.absgp_page(),
            )?;
        }

        while let Some(packet) = reader.read_packet()? {
            let stream_serial = packet.stream_serial();
            let end_info = get_end_info(&packet);
            let absgp_page = packet.absgp_page();
            writer.write_packet(packet.data, stream_serial, end_info, absgp_page)?;
        }
        // stream ended

        drop(reader);
        cursor.seek(std::io::SeekFrom::Start(0))?;
        f_in.seek(std::io::SeekFrom::Start(0))?;
        std::io::copy(&mut cursor, &mut f_in)?;

        Ok(())
    }

    pub fn write_to_path<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        self.write_to(file)
    }

    fn construct_packet_from_tag(&self) -> std::result::Result<Vec<u8>, std::num::TryFromIntError> {
        let mut output = vec![];
        // magic signature
        output.extend_from_slice("OpusTags".as_bytes());

        // encode vendor
        let vendor = &self.vendor;
        let vendor_length: u32 = vendor.len().try_into()?;
        output.extend_from_slice(&vendor_length.to_le_bytes());
        output.extend_from_slice(vendor.as_bytes());

        let mut formatted_tags = vec![];
        for (tag, values) in &self.comments {
            for value in values {
                formatted_tags.push(format!("{tag}={value}"));
            }
        }

        let num_comments: u32 = formatted_tags.len().try_into()?;
        output.extend_from_slice(&num_comments.to_le_bytes());

        for tag in formatted_tags {
            let tag_length: u32 = tag.len().try_into()?;
            output.extend_from_slice(&tag_length.to_le_bytes());
            output.extend_from_slice(tag.as_bytes());
        }

        Ok(output)
    }
}

fn get_end_info(packet: &ogg::Packet) -> PacketWriteEndInfo {
    if packet.last_in_stream() {
        PacketWriteEndInfo::EndStream
    } else if packet.last_in_page() {
        PacketWriteEndInfo::EndPage
    } else {
        PacketWriteEndInfo::NormalPacket
    }
}
