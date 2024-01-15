use coap_message::{MessageOption, MutableWritableMessage, ReadableMessage};

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

/// A trait semantically similar to TryFrom, but rather than being generic over the source, this
/// trait is specialized in being from any impl of MessageOption.
///
/// Types that implement this implicitly encode an extra piece of information, their option number:
/// Options passed in that don't have a matching number need to be ignored with a `None` return
/// value.
///
/// In passing, we also introduce a lifetime for the option (we work `from(&O)` instead of
/// `from(O)`) because this is always used with data copied out (as the MessageOption doesn't allow
/// long-term references into it anyway).
///
/// This uses an Option and not a Result, because the main way in which it is used is through
/// `take_into`, which leaves unprocessable options in the stream for later failing when critical
/// options are rejected. This pattern of usage also means that non-critical options that should
/// cause errors need to implement `TryFromOption for Result<Good, Bad>`, and raise an error of
/// their own when `Some(Bad(_))` is found.
pub trait TryFromOption: Sized {
    fn try_from(value: &impl MessageOption) -> Option<Self>;
}

/// The information model for block options when used in a way that the M flag does not matter.
///
/// In serialization, the M bit is unset and ignored (RFC7959: "MUST be set as zero and ignored on
/// reception").
struct BlockDataWithoutM {
    // Validity constraint: using only up to 20 bit
    blknum: u32,
    szx: u8,
}

/// The information model for block options when used in a way that the M flag *does* matter.
struct BlockDataWithM {
    coordinates: BlockDataWithoutM,
    m: bool,
}

impl core::ops::Deref for BlockDataWithM {
    type Target = BlockDataWithoutM;
    fn deref(&self) -> &BlockDataWithoutM {
        &self.coordinates
    }
}

/// Block1 option data (as used in either the request or the response)
pub struct Block1Data(BlockDataWithM);

/// Request data from a Block2 request
///
/// As the M flag is unused in requests, it is not captured in here (and ignored at construction).
pub struct Block2RequestData(BlockDataWithoutM);

/// Error that occurs when constructing a `Block2RequestData` from a message or an option.
///
/// It is singular and contains no details (for there is no usable action from them), but usually
/// stems from either the option being repeated or having an excessively large value.
#[derive(Debug)]
pub struct BadBlock2Option;

const M_BIT: u32 = 0x08;
const SZX_MASK: u32 = 0x07;
// Block options are only up to 3 bytes long
const BLOCK_MAX: u32 = 0xffffff;

impl BlockDataWithoutM {
    fn from_u32(o: u32) -> Option<Self> {
        if o > BLOCK_MAX {
            return None;
        }
        Some(Self {
            szx: (o & SZX_MASK) as u8,
            blknum: o >> 4,
        })
    }

    fn to_u32(&self) -> u32 {
        (self.blknum << 4) | self.szx as u32
    }

    /// Size of a single block
    pub fn size(&self) -> u16 {
        1 << (4 + self.szx)
    }

    /// Number of bytes before the indicated block
    pub fn start(&self) -> u32 {
        self.size() as u32 * self.blknum
    }
}

impl BlockDataWithM {
    fn from_u32(o: u32) -> Option<Self> {
        Some(Self {
            coordinates: BlockDataWithoutM::from_u32(o)?,
            m: o & M_BIT != 0,
        })
    }

    fn to_u32(&self) -> u32 {
        self.coordinates.to_u32() | if self.m { M_BIT } else { 0 }
    }
}

impl Block2RequestData {
    /// Extract a request block 2 value from a request message.
    ///
    /// Absence of the option is not an error and results in the default value to be returned;
    /// exceeding length or duplicate entries are an error and are indicated by returning an error,
    /// which should be responded to with a Bad Option error.
    pub fn from_message(message: &impl ReadableMessage) -> Result<Self, BadBlock2Option> {
        let mut b2options = message.options().filter(|o| o.number() == option::BLOCK2);

        match b2options.next() {
            None => Ok(Self::default()),
            Some(o) => {
                if b2options.next().is_none() {
                    Self::from_option(&o)
                } else {
                    Err(BadBlock2Option)
                }
            }
        }
    }

    /// Extract a request block 2 value from a single option. An error is indicated on a malformed
    /// (ie. overly long) option.
    ///
    /// Compared to [Block2RequestData::from_message()], this can easily be packed into a single
    /// loop that processes all options and fails on unknown critical ones; on the other hand, this
    /// does not automate the check for duplicate options.
    ///
    /// # Panics
    ///
    /// In debug mode if the option is not Block2
    pub fn from_option(option: &impl MessageOption) -> Result<Self, BadBlock2Option> {
        debug_assert!(option.number() == option::BLOCK2);
        let o: u32 = option.value_uint().ok_or(BadBlock2Option)?;
        BlockDataWithoutM::from_u32(o)
            .map(Self)
            .ok_or(BadBlock2Option)
    }

    pub fn to_option_value(&self, more: bool) -> u32 {
        self.0.to_u32() | if more { 0x08 } else { 0 }
    }

    /// Size of a single block
    pub fn size(&self) -> u16 {
        self.0.size()
    }

    /// Number of bytes before the indicated block
    pub fn start(&self) -> usize {
        self.0.start() as _
    }

    /// Return a block that has identical .start(), but a block size smaller or equal to the given
    /// one.
    ///
    /// Returns None if the given size is not expressible as a CoAP block (ie. is less than 16).
    pub fn shrink(mut self, size: u16) -> Option<Self> {
        while self.size() > size {
            if self.0.szx == 0 {
                return None;
            }
            self.0.szx -= 1;
            self.0.blknum *= 2;
        }
        Some(self)
    }
}

impl Block1Data {
    /// Number of bytes before the indicated block
    pub fn start(&self) -> usize {
        self.0.start() as _
    }

    pub fn more(&self) -> bool {
        self.0.m
    }

    pub fn to_option_value(&self) -> u32 {
        self.0.to_u32()
    }
}

impl Default for Block2RequestData {
    fn default() -> Self {
        Self(BlockDataWithoutM { szx: 6, blknum: 0 })
    }
}

impl TryFromOption for Block2RequestData {
    fn try_from(value: &impl MessageOption) -> Option<Self> {
        if value.number() != coap_numbers::option::BLOCK2 {
            return None;
        }

        Self::from_option(value).ok()
    }
}

impl TryFromOption for Block1Data {
    fn try_from(o: &impl MessageOption) -> Option<Self> {
        if o.number() != coap_numbers::option::BLOCK1 {
            return None;
        }

        Some(Self(BlockDataWithM::from_u32(o.value_uint()?)?))
    }
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
