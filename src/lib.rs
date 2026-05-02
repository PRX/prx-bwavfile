/*!
# prx-bwavfile

Rust Wave File Reader/Writer with Broadcast-WAV, MBWF, RF64, and broadcast-automation
metadata support, including MPEG/MP2 audio, the EBU `mext` chunk, and the AES46 `cart`
chunk.

Fork of [bwavfile][upstream] by Jamie Hardt and Ian Hobson, extended by PRX with
additional chunk and codec support for broadcast distribution workflows.

Refer to the individual modules for relevant documentation. For opening
and writing files begin with [WaveReader] and [WaveWriter] respectively.

## Objectives and Roadmap

This package aims to support read and writing any kind of WAV file you are likely
to encounter in a professional audio, motion picture production, broadcast, or music
production.

Apps we test against:
- Avid Pro Tools
- iZotope RX Audio Editor
- FFMpeg
- Audacity
- Sound Devices field recorders: 702T, MixPre-10 II

[upstream]: https://github.com/iluvcapra/bwavfile
*/

extern crate byteorder;
extern crate encoding;
extern crate uuid;

mod common_format;
mod errors;
mod fourcc;

mod list_form;
mod parser;

mod bext;
mod cart;
mod chunks;
mod cue;
mod fact;
mod fmt;
mod mext;

mod sample;

mod wavereader;
mod wavewriter;

pub use bext::Bext;
pub use cart::{Cart, CartTimer};
pub use common_format::{
    CommonFormat, WAVE_TAG_EXTENDED, WAVE_TAG_FLOAT, WAVE_TAG_MPEG, WAVE_TAG_PCM,
    WAVE_UUID_BFORMAT_FLOAT, WAVE_UUID_BFORMAT_PCM, WAVE_UUID_FLOAT, WAVE_UUID_MPEG, WAVE_UUID_PCM,
};
pub use cue::Cue;
pub use errors::Error;
pub use fact::Fact;
pub use fmt::{
    ADMAudioID, ChannelDescriptor, ChannelMask, ReadWavAudioData, WaveFmt, WaveFmtExtended,
    WaveFmtMpeg1,
};
pub use fourcc::FourCC;
pub use mext::Mext;
pub use sample::{Sample, I24};
pub use wavereader::{AudioFrameReader, WaveReader};
pub use wavewriter::{AudioFrameWriter, WaveWriter};
