//! Helpers around embedded-io

use core::convert::Infallible;
use extra_embedded_io_adapters::WritableCrc;
use tee_embedded_io::Tee;
use windowed_infinity::WindowedInfinity;

/// Similar to [`embedded_io_adapters::fmt::ToFmt`] but provides both interfaces.
///
/// Effectively, read this as `impl core::fmt::Write + embedded_io::Write`.
///
/// This is not generalized into `extra-embedded-io-adapters` not just because it treads on
/// `embedded-io-adapters` turf (essentially reimplementing its `ToFmt` but with the extra trait),
/// but more so because we don't *want* to make external promises: The interfaces this provides are
/// part of the block writing API, and should not just be extended because something happens in
/// another crate.
pub struct EmbeddedIoOrFmtWrite<'a>(
    pub(crate) Tee<WindowedInfinity<'a>, WritableCrc<'a, u64>, Infallible>,
);

impl embedded_io::ErrorType for EmbeddedIoOrFmtWrite<'_> {
    type Error = Infallible;
}

impl embedded_io::Write for EmbeddedIoOrFmtWrite<'_> {
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

impl core::fmt::Write for EmbeddedIoOrFmtWrite<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        embedded_io_adapters::fmt::ToFmt::new(&mut self.0).write_str(s)
    }
}
