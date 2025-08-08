//! Helpers around embedded-io

/// Similar to [`embedded_io_adapters::fmt::ToFmt`] but provides both interfaces.
///
/// This is not part of `extra-embedded-io-adapters` not just because it treads on
/// `embedded-io-adapters` turf (essentially reimplementing its `ToFmt` but with the extra trait),
/// but more so because we don't *want* to make external promises: The interfaces this provides are
/// part of the block writing API, and should not just be extended because something happens in
/// another crate.
pub(crate) struct EmbeddedIoOrFmtWrite<T: embedded_io::Write>(pub(crate) T);

impl<T: embedded_io::Write> embedded_io::ErrorType for EmbeddedIoOrFmtWrite<T> {
    type Error = T::Error;
}

impl<T: embedded_io::Write> embedded_io::Write for EmbeddedIoOrFmtWrite<T> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush()
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.0.write_all(buf)
    }

    #[inline]
    fn write_fmt(
        &mut self,
        fmt: core::fmt::Arguments<'_>,
    ) -> Result<(), embedded_io::WriteFmtError<Self::Error>> {
        self.0.write_fmt(fmt)
    }
}

impl<T: embedded_io::Write> core::fmt::Write for EmbeddedIoOrFmtWrite<T> {
    #[inline]
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        embedded_io_adapters::fmt::ToFmt::new(&mut self.0).write_str(s)
    }
}
