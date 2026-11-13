//! Private wire layouts and bounded string decoding shared by AX profiles.
//! Profile-specific validation and native status meanings stay with callers.
use crate::Error;
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;

/// Mirrors `LLControlRow` in native/controls.h, also embedded in LLWidgetRow.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct RawControl {
    pub identity: u64,
    pub key_length: usize,
    pub role_length: usize,
    pub identifier_length: usize,
    pub label_length: usize,
    pub value_length: usize,
    pub value_present: i32,
    pub enabled: i32,
    pub editable: i32,
    pub pressable: i32,
}

/// Mirrors `LLControlsSnapshot`; widgets use the same envelope.
#[repr(C)]
#[derive(Default)]
pub(crate) struct RawScope {
    pub identity: u64,
    pub revision: u64,
    pub count: usize,
    pub bytes_length: usize,
}

/// Reads packed native strings after the caller bounds the whole payload.
/// Failed reads leave the cursor unchanged; publication remains the caller's
/// responsibility after validating every row and consuming the exact payload.
pub(crate) struct PackedStrings<'a> {
    remaining: &'a [u8],
    truncated: &'static str,
    invalid_utf8: &'static str,
}
impl<'a> PackedStrings<'a> {
    pub fn new(bytes: &'a [u8], truncated: &'static str, invalid_utf8: &'static str) -> Self {
        Self {
            remaining: bytes,
            truncated,
            invalid_utf8,
        }
    }

    pub fn take(&mut self, length: usize) -> Result<String, Error> {
        if length > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        let part = self
            .remaining
            .get(..length)
            .ok_or(Error::Invalid(self.truncated))?;
        let text = std::str::from_utf8(part).map_err(|_| Error::Invalid(self.invalid_utf8))?;
        self.remaining = &self.remaining[length..];
        Ok(text.to_owned())
    }

    pub fn finish(self, trailing: &'static str) -> Result<(), Error> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid(trailing))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_strings_bound_each_read_and_preserve_failed_cursor() {
        let mut strings = PackedStrings::new("字x".as_bytes(), "short", "encoding");
        assert!(matches!(strings.take(usize::MAX), Err(Error::Bounds)));
        assert!(matches!(strings.take(5), Err(Error::Invalid("short"))));
        assert!(matches!(strings.take(1), Err(Error::Invalid("encoding"))));
        assert_eq!(strings.take(3).unwrap(), "字");
        assert_eq!(strings.take(0).unwrap(), "");
        assert_eq!(strings.take(1).unwrap(), "x");
        strings.finish("trailing").unwrap();

        let maximum = vec![b'x'; MAX_SCALAR_STRING_BYTES];
        let mut strings = PackedStrings::new(&maximum, "short", "encoding");
        assert_eq!(
            strings.take(MAX_SCALAR_STRING_BYTES).unwrap().len(),
            maximum.len()
        );
        strings.finish("trailing").unwrap();
        assert!(matches!(
            PackedStrings::new(b"x", "short", "encoding").finish("trailing"),
            Err(Error::Invalid("trailing"))
        ));
    }
}
