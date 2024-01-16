use coap_message::{MessageOption, MutableWritableMessage, ReadableMessage};
use coap_message_utils::option_value::Block2RequestData;
use coap_numbers::option;

use windowed_infinity::WindowedInfinity;

// FIXME: Move to fallible operations (but that needs more work on block2_write_with_cf, which is
// due for a larger overhaul anyway)
pub(crate) fn optconvert<O: TryFrom<u16>>(option: u16) -> O {
    option
        .try_into()
        .map_err(|_| "Response type can't express options required by handler")
        .unwrap()
}

/// Provide a writer into the response message
///
/// Anything written into the writer is put into the message's payload, and the Block2 and ETag
/// option of the message are set automatically based on what is written.
///
/// As some cleanup is required at the end of the write (eg. setting the ETag and the M flag in the
/// Block2 option), the actual writing needs to take place inside a callback.
///
/// Note that only a part of the write (that which was requested by the Block2 operation) is
/// actually persisted; the rest is discarded. When the M flag indicates that the client did not
/// obtain the full message yet, it typically sends another request that is then processed the same
/// way again, for a different "window".
///
/// The type passed in should not be relied on too much -- ideally it'd be `F: for<W:
/// core::fmt::Write> FnOnce(&mut W) -> R`, and the signature may still change in that direction.
pub fn block2_write<F, R>(
    block2: Block2RequestData,
    response: &mut impl MutableWritableMessage,
    f: F,
) -> R
where
    F: FnOnce(&mut windowed_infinity::TeeForCrc<'_, '_, u64>) -> R,
{
    block2_write_with_cf(block2, response, f, None)
}

#[derive(PartialEq)]
enum Characterization {
    Underflow,
    Inside,
    Overflow,
}

use Characterization::*;

impl Characterization {
    fn new(cursor: isize, buffer: &[u8]) -> Self {
        match usize::try_from(cursor) {
            Err(_) => Underflow,
            Ok(i) if i == buffer.len() => Inside,
            _ => Overflow,
        }
    }
}

// Not fully public because it's a crude workaround for not having zippable write injectors yet
//
// Also, this needs a rewrite for fallible writes (see optconvert and other unwrap uses).
pub(crate) fn block2_write_with_cf<F, R>(
    block2: Block2RequestData,
    response: &mut impl MutableWritableMessage,
    f: F,
    cf: Option<u16>,
) -> R
where
    F: FnOnce(&mut windowed_infinity::TeeForCrc<'_, '_, u64>) -> R,
{
    let estimated_option_size = 25; // 9 bytes ETag, up to 5 bytes Block2, up to 5 bytes Size2, 1 byte payload marker
    let payload_budget = response.available_space() - estimated_option_size;
    let block2 = block2
        .shrink(payload_budget as u16)
        .expect("Tiny buffer allocated");

    response
        .add_option(optconvert(option::ETAG), &[0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    if let Some(cf) = cf {
        if let Ok(cfopt) = option::CONTENT_FORMAT.try_into() {
            response.add_option_uint(cfopt, cf).unwrap();
        }
    }
    response
        .add_option_uint(optconvert(option::BLOCK2), block2.to_option_value(false))
        .unwrap();

    let (characterization, written, etag, ret) = {
        let full_payload = response.payload_mut_with_len(block2.size().into()).unwrap();
        let writer = WindowedInfinity::new(
            &mut full_payload[..block2.size() as usize],
            -(block2.start() as isize),
        );
        let etag = crc::Crc::<u64>::new(&crc::CRC_64_ECMA_182);
        let mut writer = writer.tee_crc64(&etag);

        let ret = f(&mut writer);

        let (writer, etag) = writer.into_windowed_and_crc();
        let written = writer.written();

        (
            Characterization::new(writer.cursor(), written),
            written.len(),
            etag.finalize().to_le_bytes(),
            ret,
        )
    };

    response.truncate(written).unwrap();
    if characterization == Underflow {
        unimplemented!("Report out-of-band seek");
    }

    response.mutate_options(|optnum, value| {
        match optnum.into() {
            option::ETAG => {
                value.copy_from_slice(&etag);
            }
            option::BLOCK2 if characterization == Overflow => {
                // set "more" flag
                value[value.len() - 1] |= 0x08;
            }
            _ => (),
        };
    });

    ret
}

/// Wrapper around a ReadableMessage that hides the Uri-Host and Uri-Path options from view
///
/// This is used by a [crate::HandlerBuilder] (in particular, its path-based [crate::ForkingHandler]) to free the
/// resources from the strange duty of skipping over a critical option they are unaware of.
// TBD: Consider removing this in favor of MaskingUriUpToPathN -- if both are used, the flash
// consumption of having both surely outweighs the runtime overhead of decrementing while going
// through the options, and the length is known in advance anyway.
pub struct MaskingUriUpToPath<'m, M: ReadableMessage>(pub &'m M);

impl<'m, M: ReadableMessage> ReadableMessage for MaskingUriUpToPath<'m, M> {
    type Code = M::Code;
    type MessageOption<'a> = M::MessageOption<'a>
    where
        Self: 'a,
    ;
    type OptionsIter<'a> = MaskingUriUpToPathIter<M::OptionsIter<'a>>
    where
        Self: 'a,
    ;

    fn options(&self) -> Self::OptionsIter<'_> {
        MaskingUriUpToPathIter(self.0.options())
    }

    fn code(&self) -> M::Code {
        self.0.code()
    }

    fn payload(&self) -> &[u8] {
        self.0.payload()
    }
}

pub struct MaskingUriUpToPathIter<I>(I);

impl<MO: MessageOption, I: Iterator<Item = MO>> Iterator for MaskingUriUpToPathIter<I> {
    type Item = MO;

    fn next(&mut self) -> Option<MO> {
        loop {
            let result = self.0.next()?;
            match result.number() {
                coap_numbers::option::URI_HOST => continue,
                coap_numbers::option::URI_PATH => continue,
                _ => return Some(result),
            }
        }
    }
}

/// Like [MaskingUriUpToPath], but only consuming a given number of Uri-Path options -- suitable
/// for ForkingTreeHandler.
pub(crate) struct MaskingUriUpToPathN<'m, M: ReadableMessage> {
    message: &'m M,
    strip_paths: usize,
}

impl<'m, M: ReadableMessage> MaskingUriUpToPathN<'m, M> {
    pub(crate) fn new(message: &'m M, strip_paths: usize) -> Self {
        Self {
            message,
            strip_paths,
        }
    }
}

impl<'m, M: ReadableMessage> ReadableMessage for MaskingUriUpToPathN<'m, M> {
    type Code = M::Code;
    type MessageOption<'a> = M::MessageOption<'a>
    where
        Self: 'a,
    ;
    type OptionsIter<'a> = MaskingUriUpToPathNIter<M::OptionsIter<'a>>
    where
        Self: 'a,
    ;

    fn options(&self) -> Self::OptionsIter<'_> {
        MaskingUriUpToPathNIter {
            inner: self.message.options(),
            remaining_strip: self.strip_paths,
        }
    }

    fn code(&self) -> M::Code {
        self.message.code()
    }

    fn payload(&self) -> &[u8] {
        self.message.payload()
    }
}

pub struct MaskingUriUpToPathNIter<I> {
    inner: I,
    remaining_strip: usize,
}

impl<MO: MessageOption, I: Iterator<Item = MO>> Iterator for MaskingUriUpToPathNIter<I> {
    type Item = MO;

    fn next(&mut self) -> Option<MO> {
        loop {
            let result = self.inner.next()?;
            match result.number() {
                coap_numbers::option::URI_HOST => continue,
                coap_numbers::option::URI_PATH if self.remaining_strip > 0 => {
                    self.remaining_strip -= 1;
                    continue;
                }
                _ => return Some(result),
            }
        }
    }
}
