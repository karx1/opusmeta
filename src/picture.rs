//! Functions and types related to handling pictures.
//!
//! This crate uses the [METADATA_BLOCK_PICTURE](https://wiki.xiph.org/VorbisComment#Cover_art)
//! proposal to encode pictures into Opus Comments.

use crate::Result;
use base64::prelude::{Engine as _, BASE64_STANDARD};
use mime_sniffer::MimeTypeSniffer;
use std::fs::OpenOptions;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use thiserror::Error;

/// Type of picture, according to the APIC picture standard.
///
/// See <https://xiph.org/flac/format.html#metadata_block_picture> for more information.
#[allow(dead_code)] // todo: change this to expect
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PictureType {
    #[default]
    Other = 0,
    FileIcon,
    OtherIcon,
    CoverFront,
    CoverBack,
    LeafletPage,
    Media,
    LeadArtist,
    Artist,
    Conductor,
    BandOrchestra,
    Composter,
    Lyricist,
    RecordingLocation,
    DuringRecording,
    DuringPerformance,
    MovieCapture,
    BrightColouredFish,
    Illustration,
    BandLogo,
    PublisherLogo,
}

impl PictureType {
    /// Create a `PictureType` from a u32. This function should really only be called from decoding
    /// functions on Picture.
    /// # Errors
    /// This function will return an error if the input number is greater than 20.
    pub fn from_u32(num: u32) -> std::result::Result<Self, PictureError> {
        if num > 20 {
            Err(PictureError::InvalidPictureType)
        } else {
            Ok(unsafe { std::mem::transmute::<u32, Self>(num) })
        }
    }
}

/// Errors that could be raised while encoding or decoding a [`Picture`].
#[derive(Debug, Clone, Error)]
pub enum PictureError {
    /// See [`PictureType::from_u32`].
    #[error("Invalid picture type")]
    InvalidPictureType,
    /// MIME Type was too long (more than [`u32::MAX`] bytes long)
    #[error("MIME type is too long (more than u32::MAX bytes long!)")]
    MimeTooLong,
    /// Description string was too long (more than [`u32::MAX`] bytes long)
    #[error("Description is too long (more than u32::MAX bytes long!)")]
    DescriptionTooLong,
    /// Picture data was too long (more than [`u32::MAX`] bytes long)
    #[error("Picture data is too long (more than u32::MAX bytes long!)")]
    DataTooLong,
    /// Failed to decode base64 data.
    #[error("Failed to decode base64 data")]
    Base64DecodeError(#[from] base64::DecodeError),
    /// Failed to sniff a mime type from a file.
    #[error("Failed to sniff mime type from file")]
    NoMimeType,
}

/// Stores picture data.
///
/// The `width`. `height`, `depth`, and `num_colors` fields should be left as
/// 0 if possible.
#[allow(dead_code)]
#[derive(Default, Clone, Debug)]
pub struct Picture {
    pub picture_type: PictureType,
    pub mime_type: String,
    pub description: String,
    pub data: Vec<u8>,
}

impl Picture {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempts to decode a Picture object from a byte slice formatted in the FLAC picture format. See
    /// <https://xiph.org/flac/format.html#metadata_block_picture> for more info.
    /// # Errors
    /// This function can error if the slice is shorter than expected, or if the system platform's
    /// usize is not big enough (See [`Error::PlatformError`](crate::Error::PlatformError) for more information).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // picture type
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let picture_type = PictureType::from_u32(u32::from_be_bytes(buffer))?;

        // mime type
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let mime_length: usize = u32::from_be_bytes(buffer).try_into()?;
        let mut buffer = vec![0; mime_length];
        cursor.read_exact(&mut buffer)?;
        let mime_type = String::from_utf8(buffer)?;

        // description
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let desc_length: usize = u32::from_be_bytes(buffer).try_into()?;
        let mut buffer = vec![0; desc_length];
        cursor.read_exact(&mut buffer)?;
        let description = String::from_utf8(buffer)?;

        // skip width, height, depth, and num_colors (4 bytes each)
        cursor.seek_relative(16)?;

        // data
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let data_length: usize = u32::from_be_bytes(buffer).try_into()?;
        let mut data = vec![0; data_length];
        cursor.read_exact(&mut data)?;

        Ok(Self {
            picture_type,
            mime_type,
            description,
            data,
        })
    }

    /// Encodes this Picture into the FLAC picture format. See
    /// <https://xiph.org/flac/format.html#metadata_block_picture> for more info.
    /// # Errors
    /// This function can error if the MIME type, Description, or picture data are too long.
    pub fn to_bytes(&self) -> std::result::Result<Vec<u8>, PictureError> {
        let mut output = vec![];

        output.extend_from_slice(&(self.picture_type as u32).to_be_bytes());
        dbg!(self.picture_type as u32);

        let mime_length: u32 = self
            .mime_type
            .len()
            .try_into()
            .map_err(|_| PictureError::MimeTooLong)?;
        output.extend_from_slice(&mime_length.to_be_bytes());
        output.extend_from_slice(self.mime_type.as_bytes());

        let desc_length: u32 = self
            .description
            .len()
            .try_into()
            .map_err(|_| PictureError::DescriptionTooLong)?;
        output.extend_from_slice(&desc_length.to_be_bytes());
        output.extend_from_slice(self.description.as_bytes());

        // write zeros for width, height, depth, and num_colors (4 bytes each)
        // because honestly i dont care about these
        let zero = [0; 16];
        output.extend_from_slice(&zero);

        let data_len: u32 = self
            .data
            .len()
            .try_into()
            .map_err(|_| PictureError::DataTooLong)?;
        output.extend_from_slice(&data_len.to_be_bytes());
        output.extend_from_slice(&self.data);

        Ok(output)
    }

    /// Encodes this Picture to the base64-encoded FLAC format, as specified by the vorbis picture
    /// proposal.
    /// # Errors
    /// This function can error if [`Picture::to_bytes`] errors.
    pub fn to_base64(&self) -> Result<String> {
        let bytes = self.to_bytes()?;
        let encoded = BASE64_STANDARD.encode(bytes);

        Ok(encoded)
    }

    /// Decodes a Picture from base64-encoded FLAC format, as specified by the vorbis picture
    /// proposal.
    /// # Errors
    /// This function can error if the input string is not valid base64, or if
    /// [`Picture::from_bytes`] errors.
    pub fn from_base64(data: &str) -> Result<Self> {
        let bytes = BASE64_STANDARD.decode(data).map_err(PictureError::from)?;
        let pic = Self::from_bytes(&bytes)?;

        Ok(pic)
    }

    /// Reads a picture from the reader. If `mime_type` is None, then this function attempts to guess
    /// the mime type based on the input data.
    /// # Errors
    /// This function can error if reading from the input fails, or if guessing the mime type from
    /// the input data fails.
    pub fn read_from<R: Read>(mut f_in: R, mime_type: Option<String>) -> Result<Self> {
        let mut output = vec![];
        f_in.read_to_end(&mut output)?;

        let mime_type = match mime_type {
            Some(s) => s,
            None => output
                .sniff_mime_type()
                .ok_or(PictureError::NoMimeType)?
                .into(),
        };

        let mut pic = Self::new();
        pic.mime_type = mime_type;
        pic.data = output;
        Ok(pic)
    }

    /// Convenience function for opening a Picture from a path. If `mime_type` is None, then this
    /// function attempts to guess the mime type based on the input data.
    /// # Errors
    /// This function can error for the same reasons as [`Picture::read_from`]
    pub fn read_from_path<P: AsRef<Path>>(path: P, mime_type: Option<String>) -> Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::read_from(file, mime_type)
    }
}
