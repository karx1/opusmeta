#![allow(clippy::module_name_repetitions)]

//! opusmeta is a Rust crate for reading and writing metadata from opus files.
//!
//! See the `read_tags` example file for basic usage.
//!
//! Unlike the more structured ID3 format, the Opus spec does not mandate a set of tag names
//! or formatting for values. However, a list of common tag names can be found
//! [here](https://xiph.org/vorbis/doc/v-comment.html).
//!
//! For reading and writing picture data, opusmeta uses the
//! [METADATA_BLOCK_PICTURE](https://wiki.xiph.org/VorbisComment#Cover_art) proposal, which is supported by common players like ffmpeg and vlc.

pub mod picture;

use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use picture::{Picture, PictureError, PictureType};
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::{Read, Seek, Write};
use std::path::Path;
use thiserror::Error;

/// Error type.
///
/// Encapsulates every error that could occur in the usage of this crate.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Failed to read an ogg packet, or the file is not an ogg file
    #[error("{0}")]
    ReadError(#[from] ogg::OggReadError),
    /// The selected file is an ogg file, but not an opus file.
    #[error("The selected file is not an opus file")]
    NotOpus,
    /// Expected a packet (for example, the comment header packet), but the stream ended early
    #[error("Expected a packet but did not receive one")]
    MissingPacket,
    /// An error occured while trying to execute an io operation. If the underlying `ErrorKind` is a
    /// [`ErrorKind::UnexpectedEof`](std::io::ErrorKind::UnexpectedEof), then it usually means that
    /// a piece of data, either an ogg packet or an encoded image, was shorter than expected by the
    /// spec.
    #[error("The comment header was malformed: {0}")]
    DataError(#[from] std::io::Error),
    /// A comment was not in TAG=VALUE format. The offending line in the comment header is provided
    /// for convenience.
    #[error("Encountered a comment which was not in TAG=VALUE format.")]
    MalformedComment(String),
    /// Expected valid UTF-8 data as mandated by the spec, but did not receive it. The underlying
    /// `FromUtf8Error` provides the offending bytes for conveniece.
    #[error("Expected valid UTF-8, but did not receive it. See the contained FromUtf8Error for the offending bytes.")]
    UTFError(#[from] std::string::FromUtf8Error),
    /// The content was too big for the opus spec (e.g. is more than [`u32::MAX`] bytes long). Since
    /// [`u32::MAX`] bytes is almost 4.3 GB, this error should almost never occur.
    #[error("The content was too big for the Opus spec")]
    TooBigError,
    /// An error occured while encoding or decoding a [`Picture`]. See [`PictureError`] for more info.
    #[error("An error occured while encoding or decoding a picture: {0}")]
    PictureError(#[from] PictureError),
    /// Raised if the platform's `usize` is smaller than 32 bits. This error is raised because
    /// the opus spec uses u32 for lengths, but Rust uses usize instead.
    #[error("This crate expects `usize` to be at least 32 bits in size.")]
    PlatformError(#[from] std::num::TryFromIntError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Stores Opus comments.
#[derive(Debug, Default)]
pub struct Tag {
    vendor: String,
    comments: HashMap<String, Vec<String>>,
}

impl Tag {
    /// Create a new tag from a vendor string and a list of comments.
    #[must_use]
    pub fn new(vendor: String, comments: Vec<(String, String)>) -> Self {
        let mut comments_map = HashMap::new();
        for (mut key, value) in comments {
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

    /// Add one entry.
    pub fn add_one(&mut self, mut tag: String, value: String) {
        tag.make_ascii_lowercase();
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.push(value.clone()))
            .or_insert(vec![value]);
    }

    /// Add multiple entries.
    pub fn add_many(&mut self, mut tag: String, mut values: Vec<String>) {
        tag.make_ascii_lowercase();
        self.comments
            .entry(tag)
            .and_modify(|v: &mut Vec<String>| v.append(&mut values))
            .or_insert(values);
    }

    /// Get all entries for a particular key, or None if no occurrences of the key exist.
    #[must_use]
    pub fn get(&self, mut tag: String) -> Option<&Vec<String>> {
        tag.make_ascii_lowercase();
        self.comments.get(&tag)
    }

    /// Remove all entries for a particular key. Optionally returns the removed values, if any.
    pub fn remove_entries(&mut self, mut tag: String) -> Option<Vec<String>> {
        tag.make_ascii_lowercase();
        self.comments.remove(&tag)
    }

    /// Gets the vendor string
    #[must_use]
    pub fn get_vendor(&self) -> &str {
        &self.vendor
    }

    /// Sets the vendor string.
    pub fn set_vendor(&mut self, new_vendor: String) {
        self.vendor = new_vendor;
    }

    /// Add a picture. If a picture with the same `PictureType` already exists, it is removed first.
    /// # Errors
    /// This function will error if [`remove_picture_type`](Self::remove_picture_type) errors, or
    /// if encoding the given data to Opus format or to base64 errors.
    pub fn add_picture(&mut self, picture: &Picture) -> Result<()> {
        let _ = self.remove_picture_type(picture.picture_type)?;
        let data = picture.to_base64()?;
        self.add_one("METADATA_BLOCK_PICTURE".to_string(), data);
        Ok(())
    }

    /// Removes a picture with the given picture type. Returns the removed picture for convenience.
    /// # Errors
    /// Although rare, this function can error if a picture with the given type is not found AND
    /// the first picture in the set is not decoded properly.
    pub fn remove_picture_type(&mut self, picture_type: PictureType) -> Result<Option<Picture>> {
        let Some(pictures) = self.comments.get_mut("metadata_block_picture") else {
            return Ok(None);
        };
        let mut index_to_remove = 0;
        for (index, data) in (*pictures).iter().enumerate() {
            if let Ok(pic) = Picture::from_base64(data) {
                if pic.picture_type == picture_type {
                    index_to_remove = index;
                }
            }
        }

        Picture::from_base64(&pictures.remove(index_to_remove)).map(Some)
    }

    /// Gets a picture which has a certain picture type, or None if there are no pictures with that
    /// type.
    #[must_use]
    pub fn get_picture_type(&self, picture_type: PictureType) -> Option<Picture> {
        let pictures = self.comments.get("metadata_block_picture")?;
        for picture in pictures {
            if let Ok(decoded) = Picture::from_base64(picture) {
                if decoded.picture_type == picture_type {
                    return Some(decoded);
                }
            }
        }

        None
    }

    /// Returns a Vec of all encoded pictures. This function will skip pictures that are encoded
    /// improperly.
    #[must_use]
    pub fn pictures(&self) -> Vec<Picture> {
        let Some(pictures_raw) = self.comments.get("metadata_block_picture") else {
            return vec![];
        };
        let mut output = vec![];
        for picture in pictures_raw {
            if let Ok(decoded) = Picture::from_base64(picture) {
                output.push(decoded);
            }
        }

        output
    }
}

impl Tag {
    /// Read a `Tag` from a reader.
    /// # Errors
    /// This function can error if:
    /// - The ogg stream is shorter than expected (e.g. doesn't include the first or second
    ///     packets)
    /// - The given reader is not an opus stream
    /// - The comment header does not include the magic signature
    /// - The comment header is shorter than mandated by the spec
    /// - The platform's usize is not at least 32 bits long
    /// - The spec mandates UTF-8, but the data is invalid unicode
    /// - A comment line is not in TAG=VALUE format.
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
            let comment = String::from_utf8(buffer.clone())?;
            let pair = comment
                .split_once('=')
                .map(|(tag, value)| (tag.to_string(), value.to_string()))
                .ok_or_else(|| Error::MalformedComment(comment))?;
            comments.push(pair);
        }
        Ok(Self::new(vendor, comments))
    }

    /// Convenience function for reading comments from a path.
    /// # Errors
    /// This function will error for the same reasons as [`read_from`](Self::read_from)
    pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        Self::read_from(file)
    }

    /// Writes tags to a writer. This function expects the writer to already contain an existing
    /// opus stream. This function reads the existing stream, copies it **into memory**, replaces the
    /// comment header, and dumps the whole stream back into the file.
    /// # Errors
    /// This function will error if:
    /// - No opus stream exists in the target
    /// - The ogg stream is shorter than expected (e.g. doesn't include the first or second
    ///     packets)
    /// - A comment in this Tag object is too big for the opus spec (some string is longer than [`u32::MAX`] bytes,
    ///     or the object contains more than [`u32::MAX`] comments)
    /// - An unspecified error occurs while reading ogg packets from the target
    /// - An error occurs while writing an ogg packet to the target
    /// - An error occurs while seeking through the target
    /// - An error occurs while copying the finished ogg stream from memory back to the target
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
            let new_pack_data = self.to_packet_data()?;
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

    /// Convenience function for writing to a path.
    /// # Errors
    /// This function will error for the same reasons as [`write_to`](Self::write_to)
    pub fn write_to_path<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        self.write_to(file)
    }

    fn to_packet_data(&self) -> Result<Vec<u8>> {
        let mut output = vec![];
        // magic signature
        output.extend_from_slice("OpusTags".as_bytes());

        // encode vendor
        let vendor = &self.vendor;
        let vendor_length: u32 = vendor.len().try_into().map_err(|_| Error::TooBigError)?;
        output.extend_from_slice(&vendor_length.to_le_bytes());
        output.extend_from_slice(vendor.as_bytes());

        let mut formatted_tags = vec![];
        for (tag, values) in &self.comments {
            for value in values {
                formatted_tags.push(format!("{tag}={value}"));
            }
        }

        let num_comments: u32 = formatted_tags
            .len()
            .try_into()
            .map_err(|_| Error::TooBigError)?;
        output.extend_from_slice(&num_comments.to_le_bytes());

        for tag in formatted_tags {
            let tag_length: u32 = tag.len().try_into().map_err(|_| Error::TooBigError)?;
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
