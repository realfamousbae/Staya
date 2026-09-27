//! Мелкие помощники для байтовых раскладок.

use crate::ProtoError;

/// Читатель со строгой проверкой границ.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        if self.buf.len() < n {
            return Err(ProtoError::Truncated);
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtoError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProtoError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ProtoError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, ProtoError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, ProtoError> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    /// Поле с префиксом длины `u16`.
    pub(crate) fn lp16(&mut self) -> Result<&'a [u8], ProtoError> {
        let len = self.u16()? as usize;
        self.take(len)
    }

    /// Поле с префиксом длины `u8`.
    pub(crate) fn lp8(&mut self) -> Result<&'a [u8], ProtoError> {
        let len = self.u8()? as usize;
        self.take(len)
    }
}

/// Дописывает поле с префиксом длины `u16`.
pub(crate) fn put_lp16(
    out: &mut Vec<u8>,
    data: &[u8],
    what: &'static str,
) -> Result<(), ProtoError> {
    let len = u16::try_from(data.len()).map_err(|_| ProtoError::TooLarge(what))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    Ok(())
}

/// Дописывает поле с префиксом длины `u8`.
pub(crate) fn put_lp8(
    out: &mut Vec<u8>,
    data: &[u8],
    what: &'static str,
) -> Result<(), ProtoError> {
    let len = u8::try_from(data.len()).map_err(|_| ProtoError::TooLarge(what))?;
    out.push(len);
    out.extend_from_slice(data);
    Ok(())
}
