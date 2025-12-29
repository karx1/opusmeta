#![allow(clippy::module_name_repetitions)]
#![doc = include_str!("../README.md")]

pub mod iter;
pub mod picture;
pub mod utils;

use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::{Read, Seek};
use std::path::Path;

use iter::{CommentsIterator, PicturesIterator};
use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use picture::{Picture, PictureError, PictureType};
use utils::StorageFile;

pub use utils::LowercaseString;

/// Error type.
///
/// Encapsulates every error that could occur in the usage of this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Failed to read an ogg packet, or the file is not an ogg file
    ReadError(ogg::OggReadError),
    /// The selected file is an ogg file, but not an opus file.
    NotOpus,
    /// Expected a packet (for example, the comment header packet), but the stream ended early
    MissingPacket,
    /// An error occured while trying to execute an io operation. If the underlying `ErrorKind` is a
    /// [`ErrorKind::UnexpectedEof`](std::io::ErrorKind::UnexpectedEof), then it usually means that
    /// a piece of data, either an ogg packet or an encoded image, was shorter than expected by the
    /// spec.
    DataError(std::io::Error),
    /// A comment was not in TAG=VALUE format. The offending line in the comment header is provided
    /// for convenience.
    MalformedComment(String),
    /// Expected valid UTF-8 data as mandated by the spec, but did not receive it. The underlying
    /// `FromUtf8Error` provides the offending bytes for conveniece.
    UTFError(std::string::FromUtf8Error),
    /// The content was too big for the opus spec (e.g. is more than [`u32::MAX`] bytes long). Since
    /// [`u32::MAX`] bytes is almost 4.3 GB, this error should almost never occur.
    TooBigError,
    /// An error occured while encoding or decoding a [`Picture`]. See [`PictureError`] for more info.
    PictureError(PictureError),
    /// Raised if the platform's `usize` is smaller than 32 bits. This error is raised because
    /// the opus spec uses u32 for lengths, but Rust uses usize instead.
    PlatformError(std::num::TryFromIntError),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadError(err) => Display::fmt(err, f),
            Self::NotOpus => f.write_str("The selected file is not an opus file"),
            Self::MissingPacket => f.write_str("Expected a packet but did not receive one"),
            Self::DataError(err) => write!(f, "The comment header was malformed: {err}"),
            Self::MalformedComment(_) => f.write_str("Encountered a comment which was not in TAG=VALUE format."),
            Self::UTFError(_) => f.write_str("Expected valid UTF-8, but did not receive it. See the contained FromUtf8Error for the offending bytes."),
            Self::TooBigError => f.write_str("The content was too big for the Opus spec"),
            Self::PictureError(err) => write!(f, "An error occured while encoding or decoding a picture: {err}"),
            Self::PlatformError(_) => f.write_str("This crate expects `usize` to be at least 32 bits in size."),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::num::TryFromIntError> for Error {
    fn from(v: std::num::TryFromIntError) -> Self {
        Self::PlatformError(v)
    }
}

impl From<PictureError> for Error {
    fn from(v: PictureError) -> Self {
        Self::PictureError(v)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(v: std::string::FromUtf8Error) -> Self {
        Self::UTFError(v)
    }
}

impl From<std::io::Error> for Error {
    fn from(v: std::io::Error) -> Self {
        Self::DataError(v)
    }
}

impl From<ogg::OggReadError> for Error {
    fn from(v: ogg::OggReadError) -> Self {
        Self::ReadError(v)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

const PICTURE_BLOCK_TAG: &str = "metadata_block_picture";

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
            comments_map.entry(key).or_insert_with(Vec::new).push(value);
        }

        Self {
            vendor,
            comments: comments_map,
        }
    }

    /// Add one entry.
    pub fn add_one(&mut self, tag: LowercaseString, value: String) {
        self.comments
            .entry(tag.0.into_owned())
            .or_default()
            .push(value);
    }

    /// Add multiple entries.
    pub fn add_many(&mut self, tag: LowercaseString, mut values: Vec<String>) {
        self.comments
            .entry(tag.0.into_owned())
            .and_modify(|v: &mut Vec<String>| v.append(&mut values))
            .or_insert(values);
    }

    /// Get all entries for a particular key, or None if no occurrences of the key exist.
    #[must_use]
    pub fn get(&self, tag: &LowercaseString) -> Option<&Vec<String>> {
        self.comments.get(tag.0.as_ref())
    }

    /// Gets the first entry for a particular key, or None if no occurences of the key exist.
    #[must_use]
    pub fn get_one(&self, tag: &LowercaseString) -> Option<&String> {
        self.comments.get(tag.0.as_ref()).and_then(|v| v.first())
    }

    /// Remove all entries for a particular key. Optionally returns the removed values, if any.
    pub fn remove_entries(&mut self, tag: &LowercaseString) -> Option<Vec<String>> {
        self.comments.remove(tag.0.as_ref())
    }

    /// Remove all entries for a particular key, inserting the given values instead.
    pub fn set_entries(
        &mut self,
        tag: LowercaseString,
        values: Vec<String>,
    ) -> Option<Vec<String>> {
        self.comments.insert(tag.0.into_owned(), values)
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
    /// This function will error  if encoding the given data to Opus format or to base64 errors.
    pub fn add_picture(&mut self, picture: &Picture) -> Result<()> {
        let _ = self.remove_picture_type(picture.picture_type)?;
        let data = picture.to_base64()?;
        self.add_one(PICTURE_BLOCK_TAG.into(), data);
        Ok(())
    }

    /// Removes a picture with the given picture type. Returns the removed picture for convenience.
    /// # Errors
    /// This function will never error.
    /// The reason it returns a Result is due to backwards compatibility reasons.
    pub fn remove_picture_type(&mut self, picture_type: PictureType) -> Result<Option<Picture>> {
        let Some(pictures) = self.comments.get_mut(PICTURE_BLOCK_TAG) else {
            return Ok(None);
        };

        for (index, data) in (*pictures).iter().enumerate() {
            if let Ok(pic) = Picture::from_base64(data)
                && pic.picture_type == picture_type
            {
                pictures.remove(index);
                return Ok(Some(pic));
            }
        }

        Ok(None)
    }

    /// Gets a picture which has a certain picture type, or None if there are no pictures with that
    /// type.
    #[must_use]
    pub fn get_picture_type(&self, picture_type: PictureType) -> Option<Picture> {
        let pictures = self.comments.get(PICTURE_BLOCK_TAG)?;
        for picture in pictures {
            if let Ok(decoded) = Picture::from_base64(picture)
                && decoded.picture_type == picture_type
            {
                return Some(decoded);
            }
        }

        None
    }

    /// Returns whether any pictures are stored within the opus file.
    #[must_use]
    pub fn has_pictures(&self) -> bool {
        self.comments.contains_key(PICTURE_BLOCK_TAG)
    }

    /// Returns a Vec of all encoded pictures. This function will skip pictures that are encoded
    /// improperly.
    #[must_use]
    pub fn pictures(&self) -> Vec<Picture> {
        self.iter_pictures()
            .map_or_else(Vec::new, |iter| iter.filter_map(Result::ok).collect())
    }
}

impl Tag {
    /// Read a `Tag` from a reader.
    /// # Errors
    /// This function can error if:
    /// - The ogg stream is shorter than expected (e.g. doesn't include the first or second packets)
    /// - The given reader is not an opus stream
    /// - The comment header does not include the magic signature
    /// - The comment header is shorter than mandated by the spec
    /// - The platform's usize is not at least 32 bits long
    /// - The spec mandates UTF-8, but the data is invalid unicode
    /// - A comment line is not in TAG=VALUE format.
    pub fn read_from<R: Read + Seek>(f_in: R) -> Result<Self> {
        let mut reader = PacketReader::new(f_in);
        let first_packet = reader.read_packet()?.ok_or(Error::MissingPacket)?;
        if !first_packet.data.starts_with(b"OpusHead") {
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
                .ok_or(Error::MalformedComment(comment))?;
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
    /// - The ogg stream is shorter than expected (e.g. doesn't include the first or second packets)
    /// - A comment in this Tag object is too big for the opus spec (some string is longer than [`u32::MAX`] bytes,
    ///   or the object contains more than [`u32::MAX`] comments)
    /// - An unspecified error occurs while reading ogg packets from the target
    /// - An error occurs while writing an ogg packet to the target
    /// - An error occurs while seeking through the target
    /// - An error occurs while copying the finished ogg stream from memory back to the target
    pub fn write_to<F: StorageFile>(&self, mut f_in: F) -> Result<()> {
        let mut f_out_raw: Vec<u8> = vec![];
        let mut cursor = Cursor::new(&mut f_out_raw);

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

        f_in.seek(std::io::SeekFrom::Start(0))?;
        f_in.set_len(f_out_raw.len() as u64)?;
        f_in.write_all(&f_out_raw)?;

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
        output.extend_from_slice(b"OpusTags");

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

impl Tag {
    /// An iterator over the comments of an opus file, excluding pictures.
    ///
    /// See [`CommentsIterator`] for more info.
    #[must_use]
    pub fn iter_comments(&self) -> CommentsIterator<'_> {
        CommentsIterator {
            comments_iter: self.comments.iter().filter(|c| c.0 != PICTURE_BLOCK_TAG),
        }
    }

    /// An iterator over the images embedded in an opus file.
    ///
    /// See [`PicturesIterator`] for more info.
    #[must_use]
    pub fn iter_pictures(&self) -> Option<PicturesIterator<'_>> {
        self.comments
            .get(PICTURE_BLOCK_TAG)
            .map(|pict_vec| PicturesIterator {
                pictures_iter: pict_vec.iter(),
            })
    }

    /// An iterator over the comment keys of an opus file, excluding the picture block key.
    ///
    /// The iterator Item is `&'a str`.
    /// This iterator immutably borrows the tags stored in the [`Tag`] struct.
    /// To check whether the set of tags contains pictures, see [`has_pictures`](Tag::has_pictures).
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.comments
            .keys()
            .filter(|k| *k != PICTURE_BLOCK_TAG)
            .map(AsRef::as_ref)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_image_with_no_matching_type() {
        // File contains exactly one image with CoverFront type.
        let mut tag =
            Tag::read_from_path("testfiles/silence_cover.opus").expect("Failed to open testfile");

        // Removing different type should not remove anything
        let remove_result = tag.remove_picture_type(PictureType::Media);
        assert!(matches!(remove_result, Ok(None)));
    }

    #[test]
    fn test_remove_image_when_empty() {
        // File contains exactly one image with CoverFront type.
        let mut tag =
            Tag::read_from_path("testfiles/silence_cover.opus").expect("Failed to open testfile");

        // Removing matching type should remove picture
        let remove_result = tag.remove_picture_type(PictureType::CoverFront);
        assert!(matches!(remove_result, Ok(Some(_))));

        // Removing anything with no pictures left should not return anything
        let remove_result = tag.remove_picture_type(PictureType::CoverFront);
        assert!(matches!(remove_result, Ok(None)));
    }
}
