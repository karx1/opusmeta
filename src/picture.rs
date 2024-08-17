use crate::Result;
use std::io::{Cursor, Read};
use thiserror::Error;

#[allow(dead_code)] // todo: change this to expect
#[derive(Default, Debug, Clone, Copy)]
#[repr(u32)]
pub enum PictureType {
    #[default]
    Other = 0,
    Icon,
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
    pub fn from_u32(num: u32) -> std::result::Result<Self, PictureDecodeError> {
        if num > 20 {
            Err(PictureDecodeError::InvalidPictureType)
        } else {
            Ok(unsafe { std::mem::transmute::<u32, PictureType>(num) })
        }
    }
}

#[derive(Debug, Copy, Clone, Error)]
pub enum PictureDecodeError {
    #[error("Invalid picture type")]
    InvalidPictureType,
    #[error("MIME type is too long (more than u32::MAX bytes long!)")]
    MimeTooLong,
    #[error("Description is too long (more than u32::MAX bytes long!)")]
    DescriptionTooLong,
}

#[allow(dead_code)]
#[derive(Default, Clone, Debug)]
pub struct Picture {
    picture_type: PictureType,
    mime_type: String,
    description: String,
    width: u32,
    height: u32,
    depth: u32,
    num_colors: u32,
    data: Vec<u8>,
}

impl Picture {
    pub fn new() -> Self {
        Self::default()
    }

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

        // width
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let width = u32::from_be_bytes(buffer);

        // height
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let height = u32::from_be_bytes(buffer);

        // depth
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let depth = u32::from_be_bytes(buffer);

        // num_colors
        let mut buffer = [0; 4];
        cursor.read_exact(&mut buffer)?;
        let num_colors = u32::from_be_bytes(buffer);

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
            width,
            height,
            depth,
            num_colors,
            data,
        })
    }
}
