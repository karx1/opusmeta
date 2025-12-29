//! This module contains utility types/traits.

use std::borrow::Cow;
use std::fmt::Display;
use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};
use std::ops::Deref;

/// A lowercase String. Holds a [`Cow<str>`] internally.
#[derive(Debug, Clone)]
pub struct LowercaseString<'a>(pub(crate) Cow<'a, str>);

impl Deref for LowercaseString<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for LowercaseString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl<'a> LowercaseString<'a> {
    /// Create a new `LowercaseString`. This will only allocate if the passed
    /// string isn't lowercase.
    #[must_use]
    pub fn new_from_str(str: &'a str) -> Self {
        if str.chars().any(|c| c.is_ascii_uppercase()) {
            Self(Cow::Owned(str.to_ascii_lowercase()))
        } else {
            Self(Cow::Borrowed(str))
        }
    }

    /// Create a new `LowercaseString`. If the string isn't already lowercase,
    /// this will modify the existing buffer without allocating.
    #[must_use]
    pub fn new_from_string(mut str: String) -> Self {
        str.make_ascii_lowercase();
        Self(Cow::Owned(str))
    }

    /// Try to create a new `LowercaseString`. This returns `None` if the passed
    /// string isn't lowercase.
    #[must_use]
    pub const fn try_from_str(str: &'a str) -> Option<Self> {
        // for loops and iterator/trait methods aren't const stable yet.
        let mut i = 0;
        // len() works here because it returns length in bytes
        while i < str.len() {
            if str.as_bytes()[i].is_ascii_uppercase() {
                return None;
            }
            i += 1;
        }

        Some(Self(Cow::Borrowed(str)))
    }
}

impl<S: AsRef<str>> From<S> for LowercaseString<'static> {
    fn from(str: S) -> Self {
        Self::new_from_string(str.as_ref().to_string())
    }
}

/// A trait representing a file-like reader/writer.
///
/// This trait is the combination of the [`std::io`]
/// stream traits with an additional method to resize the file.
pub trait StorageFile: Read + Write + Seek {
    /// Resize the file. This method behaves the same as
    /// [`File::set_len`].
    /// # Errors
    /// See [`File::set_len`] for reasons this function could error.
    fn set_len(&mut self, new_size: u64) -> crate::Result<()>;
}

impl<T: StorageFile> StorageFile for &mut T {
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        T::set_len(self, new_size)
    }
}

impl StorageFile for File {
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        Ok(File::set_len(self, new_size)?)
    }
}

impl StorageFile for &File {
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        Ok(File::set_len(self, new_size)?)
    }
}

impl StorageFile for Cursor<Vec<u8>> {
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        // This will only cause a problem on 32-bit platforms
        // But if we run into this problem then the Vec is too big for memory anyway
        #[allow(clippy::cast_possible_truncation)]
        self.get_mut().resize(new_size as usize, 0);
        Ok(())
    }
}

impl StorageFile for Cursor<&mut Vec<u8>> {
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        // This will only cause a problem on 32-bit platforms
        // But if we run into this problem then the Vec is too big for memory anyway
        #[allow(clippy::cast_possible_truncation)]
        self.get_mut().resize(new_size as usize, 0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dont_allocate_already_lowercase_str() {
        let lower = LowercaseString::new_from_str("adsf-adsf");
        assert!(matches!(lower.0, Cow::Borrowed(_)));
    }
}
