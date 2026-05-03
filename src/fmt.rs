use crate::common_format::{CommonFormat, WAVE_UUID_BFORMAT_PCM, WAVE_UUID_PCM};
use crate::mpeg::{ChannelMode, MpegInfo, MpegVersion};
use crate::Sample;

/// `MPEG1WAVEFORMAT.fwHeadModeExt`: bitmask of joint-stereo subbands
/// in use. `1u16 << info.mode_extension` for joint-stereo files,
/// `0` otherwise.
fn head_mode_ext(info: &MpegInfo) -> u16 {
    if matches!(info.channel_mode, ChannelMode::JointStereo) {
        1u16 << info.mode_extension
    } else {
        0
    }
}

/// `MPEG1WAVEFORMAT.fwHeadFlags`: bitmask combining the various
/// per-frame flags into the format-chunk-level encoding per Microsoft
/// ACM constants.
fn head_flags(info: &MpegInfo) -> u16 {
    let mut flags: u16 = 0;
    if info.private_bit {
        flags |= 0x0001; // ACM_MPEG_PRIVATEBIT
    }
    if info.copyright {
        flags |= 0x0002; // ACM_MPEG_COPYRIGHT
    }
    if info.original {
        flags |= 0x0004; // ACM_MPEG_ORIGINALHOME
    }
    if info.error_protection {
        flags |= 0x0008; // ACM_MPEG_PROTECTIONBIT
    }
    if matches!(info.version, MpegVersion::V1) {
        flags |= 0x0010; // ACM_MPEG_ID_MPEG1
    }
    flags
}

use std::io::Cursor;
use uuid::Uuid;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;

// Need more test cases for ADMAudioID
#[allow(dead_code)]

/// ADM Audio ID record.
///
/// This structure relates a channel in the wave file to either a common ADM
/// channel definition or further definition in the WAV file's ADM metadata
/// chunk.
///
/// An individual channel in a WAV file can have multiple Audio IDs in an ADM
/// `AudioProgramme`.
///
/// See BS.2088-1 § 8, also BS.2094, also blahblahblah...
#[derive(Debug)]
pub struct ADMAudioID {
    pub track_uid: [char; 12],
    pub channel_format_ref: [char; 14],
    pub pack_ref: [char; 11],
}

/// Describes a single channel in a WAV file.
///
/// This information is correlated from the Wave format ChannelMap field and
/// the `chna` chunk, if present.
#[derive(Debug)]
pub struct ChannelDescriptor {
    /// Index, the offset of this channel's samples in one frame.
    pub index: u16,

    /// Channel assignment
    ///
    /// This is either implied (in the case of mono or stereo wave files) or
    /// explicitly given in `WaveFormatExtentended` for files with more tracks.
    pub speaker: ChannelMask,

    /// ADM audioTrackUIDs
    pub adm_track_audio_ids: Vec<ADMAudioID>,
}

/// A bitmask indicating which channels are present in
/// the file.
///
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelMask {
    DirectOut = 0x0,
    FrontLeft = 0x1,
    FrontRight = 0x2,
    FrontCenter = 0x4,
    LowFrequency = 0x8,
    BackLeft = 0x10,
    BackRight = 0x20,
    FrontCenterLeft = 0x40,
    FrontCenterRight = 0x80,
    BackCenter = 0x100,
    SideLeft = 0x200,
    SideRight = 0x400,
    TopCenter = 0x800,
    TopFrontLeft = 0x1000,
    TopFrontCenter = 0x2000,
    TopFrontRight = 0x4000,
    TopBackLeft = 0x8000,
    TopBackCenter = 0x10000,
    TopBackRight = 0x20000,
}

impl From<u32> for ChannelMask {
    fn from(value: u32) -> Self {
        match value {
            0x1 => Self::FrontLeft,
            0x2 => Self::FrontRight,
            0x4 => Self::FrontCenter,
            0x8 => Self::LowFrequency,
            0x10 => Self::BackLeft,
            0x20 => Self::BackRight,
            0x40 => Self::FrontCenterLeft,
            0x80 => Self::FrontCenterRight,
            0x100 => Self::BackCenter,
            0x200 => Self::SideLeft,
            0x400 => Self::SideRight,
            0x800 => Self::TopCenter,
            0x1000 => Self::TopFrontLeft,
            0x2000 => Self::TopFrontCenter,
            0x4000 => Self::TopFrontRight,
            0x8000 => Self::TopBackLeft,
            0x10000 => Self::TopBackCenter,
            0x20000 => Self::TopBackRight,
            _ => Self::DirectOut,
        }
    }
}

impl ChannelMask {
    pub fn channels(input_mask: u32, channel_count: u16) -> Vec<ChannelMask> {
        let reserved_mask = 0xfff2_0000_u32;
        if (input_mask & reserved_mask) > 0 {
            vec![ChannelMask::DirectOut; channel_count as usize]
        } else {
            (0..18)
                .map(|i| 1 << i)
                .filter(|mask| mask & input_mask > 0)
                .map(ChannelMask::from)
                .collect()
        }
    }
}

/**
 * Extended Wave Format
 *
 * Resources:
 * * [WAVEFORMATEXTENSIBLE structure](https://docs.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatextensible)
 */
#[derive(Debug, Copy, Clone)]
pub struct WaveFmtExtended {
    /// Valid bits per sample
    pub valid_bits_per_sample: u16,

    /// Channel mask
    ///
    /// Identifies the speaker assignment for each channel in the file
    pub channel_mask: u32,

    /// Codec GUID
    ///
    /// Identifies the codec of the audio stream
    pub type_guid: Uuid,
}

/// MPEG-1 Wave Format extension (`MPEG1WAVEFORMAT`).
///
/// Present in WAVE files whose [`WaveFmt::tag`] is `0x0050` (MPEG-1
/// audio, including MPEG-1 Layer II / MP2), as defined by EBU Tech 3285
/// Supplement 1. Carries MPEG-specific framing information that the
/// standard `fmt` chunk cannot express.
///
/// Mutually exclusive with [`WaveFmtExtended`]: only one of
/// [`WaveFmt::extended_format`] and [`WaveFmt::mpeg1_format`] is ever
/// `Some` for a given file. The `tag` field selects which.
///
/// All fields are little-endian, despite the MPEG bitstream itself
/// being big-endian — the WAVE container places this struct in RIFF
/// byte order.
///
/// ## Resources
/// - [EBU Tech 3285 Supplement 1](https://tech.ebu.ch/docs/tech/tech3285s1.pdf), §2.1
/// - [MPEG1WAVEFORMAT](https://learn.microsoft.com/en-us/previous-versions/dd757717(v=vs.85))
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WaveFmtMpeg1 {
    /// MPEG audio layer flag (`ACM_MPEG_LAYER1`=1, `LAYER2`=2, `LAYER3`=4).
    pub head_layer: u16,

    /// Average bit rate in **bits per second** (not kbps).
    pub head_bit_rate: u32,

    /// Channel mode flag (`ACM_MPEG_STEREO`=1, `JOINTSTEREO`=2,
    /// `DUALCHANNEL`=4, `SINGLECHANNEL`=8).
    pub head_mode: u16,

    /// Mode extension (only meaningful when `head_mode` indicates
    /// joint stereo).
    pub head_mode_ext: u16,

    /// Audio emphasis flag (per ACM, 1-indexed: 1=none, 2=50/15µs,
    /// 3=reserved, 4=CCIT J.17).
    pub head_emphasis: u16,

    /// Bitfield of per-frame flags (CRC, copyright, original,
    /// padding, etc.).
    pub head_flags: u16,

    /// Low 32 bits of the presentation time stamp (PTS), in 90 kHz
    /// units.
    pub pts_low: u32,

    /// High 32 bits of the presentation time stamp (PTS), in 90 kHz
    /// units.
    pub pts_high: u32,
}

///
/// WAV file data format record.
///
/// The `fmt` record contains essential information describing the binary
/// structure of the data segment of the WAVE file, such as sample
/// rate, sample binary format, channel count, etc.
///
///
/// ## Resources
///
/// ### Implementation of Wave format `fmt` chunk
/// - [MSDN WAVEFORMATEX](https://docs.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-waveformatex)
/// - [MSDN WAVEFORMATEXTENSIBLE](https://docs.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatextensible)
///
/// ### Other resources
/// - [RFC 3261][rfc3261] (June 1998) "WAVE and AVI Codec Registries"
/// - [Sampler Metadata](http://www.piclist.com/techref/io/serial/midi/wave.html)
/// - [Audio File Format Specifications](http://www-mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/WAVE.html) (September 2022) Prof. Peter Kabal, MMSP Lab, ECE, McGill University
/// - [Multimedia Programming Interface and Data Specifications 1.0](http://www-mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/Docs/riffmci.pdf)
///    (August 1991), IBM Corporation and Microsoft Corporation
///
/// [rfc3261]: https://tools.ietf.org/html/rfc2361

#[derive(Debug, Copy, Clone)]
pub struct WaveFmt {
    /// A tag identifying the codec in use.
    ///
    /// If this is 0xFFFE, the codec will be identified by a GUID
    /// in [`extended_format`](WaveFmt::extended_format).
    pub tag: u16,

    /// Count of audio channels in each frame
    pub channel_count: u16,

    /// Playback rate of the audio data
    ///
    /// In frames per second.
    pub sample_rate: u32,

    /// Count of bytes per second
    ///
    /// By rule, this is `block_alignment * sample_rate`
    pub bytes_per_second: u32,

    /// Count of bytes per audio frame
    ///
    /// By rule, this is `channel_count * bits_per_sample / 8`
    pub block_alignment: u16,

    /// Count of bits stored in the file per sample
    ///
    /// By rule, `bits_per_sample % 8 == 0` for Broadcast-Wave files.
    ///
    /// Modern clients will encode
    /// unusual sample sizes in normal byte sizes but will set the valid_bits
    /// flag in extended format record.
    ///
    /// Generally speaking this will be true for all modern wave files, though
    /// there was an historical "packed" stereo format of 20 bits per sample,
    /// 5 bytes per frame, 5 bytes block alignment.
    pub bits_per_sample: u16,

    /// Extended format description
    ///
    /// Additional format metadata if channel_count is greater than 2,
    /// or if certain codecs are used.
    ///
    /// Mutually exclusive with [`mpeg1_format`](Self::mpeg1_format):
    /// at most one of these is `Some` for a given file. `tag == 0xFFFE`
    /// selects this variant.
    pub extended_format: Option<WaveFmtExtended>,

    /// MPEG-1 audio format extension (`MPEG1WAVEFORMAT`).
    ///
    /// Present when `tag == 0x0050` (MPEG-1 audio, e.g. MP2). Defined
    /// by EBU Tech 3285 Supplement 1. Mutually exclusive with
    /// [`extended_format`](Self::extended_format).
    pub mpeg1_format: Option<WaveFmtMpeg1>,
}

impl WaveFmt {
    pub fn valid_bits_per_sample(&self) -> u16 {
        if let Some(ext) = self.extended_format {
            ext.valid_bits_per_sample
        } else {
            self.bits_per_sample
        }
    }

    /// MPEG-1 format extension if this `fmt` chunk describes MPEG-1 audio.
    ///
    /// Returns `Some` for files where `tag == 0x0050`, `None` otherwise.
    pub fn mpeg1_extension(&self) -> Option<&WaveFmtMpeg1> {
        self.mpeg1_format.as_ref()
    }

    /// Build a [`WaveFmt`] for an MPEG-1 audio stream from parsed
    /// frame info.
    ///
    /// Sets `tag = 0x0050`, populates [`mpeg1_format`](Self::mpeg1_format)
    /// from the provided [`MpegInfo`], and computes `bytes_per_second`
    /// and `block_alignment` per EBU Tech 3285 Supplement 1.
    /// `bits_per_sample` is set to `0xFFFF` per the EBU sentinel for
    /// MPEG audio.
    pub fn new_mpeg1(info: &MpegInfo) -> Self {
        WaveFmt {
            tag: 0x0050,
            channel_count: info.num_channels,
            sample_rate: info.sample_rate,
            bytes_per_second: info.bit_rate * 1000 / 8,
            block_alignment: info.frame_size as u16,
            bits_per_sample: 0xFFFF,
            extended_format: None,
            mpeg1_format: Some(WaveFmtMpeg1 {
                head_layer: info.layer.head_layer(),
                head_bit_rate: info.bit_rate * 1000,
                head_mode: info.channel_mode.head_mode(),
                head_mode_ext: head_mode_ext(info),
                head_emphasis: info.emphasis as u16 + 1,
                head_flags: head_flags(info),
                pts_low: 0,
                pts_high: 0,
            }),
        }
    }

    /// Create a new integer PCM format for a monoaural audio stream.
    pub fn new_pcm_mono(sample_rate: u32, bits_per_sample: u16) -> Self {
        Self::new_pcm_multichannel(sample_rate, bits_per_sample, 0x4)
    }

    /// Create a new integer PCM format for a standard Left-Right stereo audio
    /// stream.
    pub fn new_pcm_stereo(sample_rate: u32, bits_per_sample: u16) -> Self {
        Self::new_pcm_multichannel(sample_rate, bits_per_sample, 0x3)
    }

    /// Create a new integer PCM format for ambisonic b-format.
    pub fn new_pcm_ambisonic(sample_rate: u32, bits_per_sample: u16, channel_count: u16) -> Self {
        let container_bits_per_sample = bits_per_sample + (bits_per_sample % 8);
        let container_bytes_per_sample = container_bits_per_sample / 8;

        WaveFmt {
            tag: 0xFFFE,
            channel_count,
            sample_rate,
            bytes_per_second: container_bytes_per_sample as u32
                * sample_rate
                * channel_count as u32,
            block_alignment: container_bytes_per_sample * channel_count,
            bits_per_sample: container_bits_per_sample,
            extended_format: Some(WaveFmtExtended {
                valid_bits_per_sample: bits_per_sample,
                channel_mask: ChannelMask::DirectOut as u32,
                type_guid: WAVE_UUID_BFORMAT_PCM,
            }),
            mpeg1_format: None,
        }
    }

    /// Create a new integer PCM format [WaveFmt] with a custom channel bitmap.
    ///
    /// The order of [channels](WaveFmt::channels) is not important. When reading or writing
    /// audio frames you must use the standard multichannel order for Wave
    /// files, the numerical order of the cases of [ChannelMask].
    pub fn new_pcm_multichannel(
        sample_rate: u32,
        bits_per_sample: u16,
        channel_bitmap: u32,
    ) -> Self {
        let container_bits_per_sample = bits_per_sample + (bits_per_sample % 8);
        let container_bytes_per_sample = container_bits_per_sample / 8;

        let channel_count: u16 = (0..=31).fold(0u16, |accum, n| {
            accum + (0x1 & (channel_bitmap >> n) as u16)
        });

        let result: (u16, Option<WaveFmtExtended>) = match channel_bitmap {
            ch if bits_per_sample != container_bits_per_sample => (
                0xFFFE,
                Some(WaveFmtExtended {
                    valid_bits_per_sample: bits_per_sample,
                    channel_mask: ch,
                    type_guid: WAVE_UUID_PCM,
                }),
            ),
            0b0100 => (0x0001, None),
            0b0011 => (0x0001, None),
            ch => (
                0xFFFE,
                Some(WaveFmtExtended {
                    valid_bits_per_sample: bits_per_sample,
                    channel_mask: ch,
                    type_guid: WAVE_UUID_PCM,
                }),
            ),
        };

        let (tag, extformat) = result;

        WaveFmt {
            tag,
            channel_count,
            sample_rate,
            bytes_per_second: container_bytes_per_sample as u32
                * sample_rate
                * channel_count as u32,
            block_alignment: container_bytes_per_sample * channel_count,
            bits_per_sample: container_bits_per_sample,
            extended_format: extformat,
            mpeg1_format: None,
        }
    }

    /// Format or codec of the file's audio data.
    ///
    /// The [CommonFormat] unifies the format tag and the format extension GUID. Use this
    /// method to determine the codec.
    pub fn common_format(&self) -> CommonFormat {
        CommonFormat::make(self.tag, self.extended_format.map(|ext| ext.type_guid))
    }

    /// Create a frame buffer sized to hold `length` frames for a reader or
    /// writer
    ///
    /// This is a conveneince method that creates a `Vec<i32>` with
    /// as many elements as there are channels in the underlying stream.
    pub fn create_frame_buffer<S: Sample>(&self, length: usize) -> Vec<S> {
        vec![S::EQUILIBRIUM; self.channel_count as usize * length]
    }

    /// Create a raw byte buffer to hold `length` blocks from a reader or
    /// writer
    pub fn create_raw_buffer(&self, length: usize) -> Vec<u8> {
        vec![0u8; self.block_alignment as usize * length]
    }

    /// Read bytes into frames
    pub fn unpack_frames(&self, from_bytes: &[u8], into_frames: &mut [i32]) {
        let mut rdr = Cursor::new(from_bytes);
        for frame in into_frames {
            *frame = match (self.valid_bits_per_sample(), self.bits_per_sample) {
                (0..=8,8) => rdr.read_u8().unwrap() as i32 - 0x80_i32, // EBU 3285 §A2.2
                (9..=16,16) => rdr.read_i16::<LittleEndian>().unwrap() as i32,
                (10..=24,24) => rdr.read_i24::<LittleEndian>().unwrap(),
                (25..=32,32) => rdr.read_i32::<LittleEndian>().unwrap(),
                (b,_)=> panic!("Unrecognized integer format, bits per sample {}, channels {}, block_alignment {}", 
                    b, self.channel_count, self.block_alignment)
            }
        }
    }

    /// Channel descriptors for each channel.
    pub fn channels(&self) -> Vec<ChannelDescriptor> {
        match self.channel_count {
            1 => vec![ChannelDescriptor {
                index: 0,
                speaker: ChannelMask::FrontCenter,
                adm_track_audio_ids: vec![],
            }],
            2 => vec![
                ChannelDescriptor {
                    index: 0,
                    speaker: ChannelMask::FrontLeft,
                    adm_track_audio_ids: vec![],
                },
                ChannelDescriptor {
                    index: 1,
                    speaker: ChannelMask::FrontRight,
                    adm_track_audio_ids: vec![],
                },
            ],
            x if x > 2 => {
                let channel_mask = self.extended_format.map(|x| x.channel_mask).unwrap_or(0);
                let channels = ChannelMask::channels(channel_mask, self.channel_count);
                let channels_expanded = channels
                    .iter()
                    .chain(std::iter::repeat(&ChannelMask::DirectOut));

                (0..self.channel_count)
                    .zip(channels_expanded)
                    .map(|(n, chan)| ChannelDescriptor {
                        index: n,
                        speaker: *chan,
                        adm_track_audio_ids: vec![],
                    })
                    .collect()
            }
            x => panic!("Channel count ({}) was illegal!", x),
        }
    }
}

pub trait ReadWavAudioData {
    /// Read audio data from the receiver as interleaved [i32] samples.
    fn read_i32_frames(
        &mut self,
        format: WaveFmt,
        into: &mut [i32],
    ) -> Result<usize, std::io::Error>;
    fn read_f32_frames(
        &mut self,
        format: WaveFmt,
        into: &mut [f32],
    ) -> Result<usize, std::io::Error>;
}

impl<T> ReadWavAudioData for T
where
    T: std::io::Read,
{
    /// # Panics:
    /// * If the format's [valid bits per sample](WaveFmt::valid_bits_per_sample) is
    ///   not compatible with the format's [bits per sample](WaveFmt::bits_per_sample).
    fn read_i32_frames(
        &mut self,
        format: WaveFmt,
        into: &mut [i32],
    ) -> Result<usize, std::io::Error> {
        assert!(into.len() % format.channel_count as usize == 0);

        for frame in into {
            *frame = match (format.valid_bits_per_sample(), format.bits_per_sample) {
                (0..=8,8) => self.read_u8().unwrap() as i32 - 0x80_i32, // EBU 3285 §A2.2
                (9..=16,16) => self.read_i16::<LittleEndian>().unwrap() as i32,
                (10..=24,24) => self.read_i24::<LittleEndian>().unwrap(),
                (25..=32,32) => self.read_i32::<LittleEndian>().unwrap(),
                (b,_)=> panic!("Unrecognized integer format, bits per sample {}, channels {}, block_alignment {}", 
                    b, format.channel_count, format.block_alignment)
            }
        }

        todo!()
    }
    fn read_f32_frames(
        &mut self,
        format: WaveFmt,
        into: &mut [f32],
    ) -> Result<usize, std::io::Error> {
        assert!(into.len() % format.channel_count as usize == 0);
        todo!()
    }
}

trait WriteWavAudioData {
    fn write_i32_frames(&mut self, format: WaveFmt, from: &[i32]) -> Result<usize, std::io::Error>;
    fn write_f32_frames(&mut self, format: WaveFmt, from: &[f32]) -> Result<usize, std::io::Error>;
}

impl<T> WriteWavAudioData for T
where
    T: std::io::Write,
{
    fn write_i32_frames(&mut self, _format: WaveFmt, _: &[i32]) -> Result<usize, std::io::Error> {
        todo!()
    }
    fn write_f32_frames(&mut self, _format: WaveFmt, _: &[f32]) -> Result<usize, std::io::Error> {
        todo!()
    }
}

#[cfg(test)]
mod mpeg1_tests {
    use super::*;
    use crate::chunks::{ReadBWaveChunks, WriteBWaveChunks};
    use std::io::{Cursor, Seek, SeekFrom};

    fn sample_mpeg1_fmt() -> WaveFmt {
        WaveFmt {
            tag: 0x0050,
            channel_count: 2,
            sample_rate: 44100,
            bytes_per_second: 32000,
            block_alignment: 836,
            // EBU 3285-S1 sentinel for MPEG audio: bits_per_sample is
            // typically 0xFFFF (or 0; both are seen in the wild).
            bits_per_sample: 0xFFFF,
            extended_format: None,
            mpeg1_format: Some(WaveFmtMpeg1 {
                head_layer: 2, // ACM_MPEG_LAYER2 = 2
                head_bit_rate: 256_000,
                head_mode: 1, // ACM_MPEG_STEREO = 1
                head_mode_ext: 0,
                head_emphasis: 1, // ACM_MPEG_NONE = 1
                head_flags: 0,
                pts_low: 0,
                pts_high: 0,
            }),
        }
    }

    #[test]
    fn mpeg1_fmt_round_trip_via_chunks_traits() {
        let original = sample_mpeg1_fmt();
        let mut buf = Cursor::new(Vec::<u8>::new());
        buf.write_wave_fmt(&original).unwrap();
        // Base 16 + cb_size 2 + 22-byte MPEG-1 extension = 40 bytes
        assert_eq!(buf.get_ref().len(), 40);
        buf.seek(SeekFrom::Start(0)).unwrap();
        let parsed = buf.read_wave_fmt().unwrap();

        assert_eq!(parsed.tag, original.tag);
        assert_eq!(parsed.channel_count, original.channel_count);
        assert_eq!(parsed.sample_rate, original.sample_rate);
        assert_eq!(parsed.bytes_per_second, original.bytes_per_second);
        assert_eq!(parsed.block_alignment, original.block_alignment);
        assert_eq!(parsed.bits_per_sample, original.bits_per_sample);
        assert!(parsed.extended_format.is_none());
        assert_eq!(parsed.mpeg1_format, original.mpeg1_format);
    }

    #[test]
    fn mpeg1_extension_accessor() {
        let f = sample_mpeg1_fmt();
        assert!(f.mpeg1_extension().is_some());
        assert_eq!(f.mpeg1_extension().unwrap().head_layer, 2);

        let pcm = WaveFmt::new_pcm_mono(48000, 24);
        assert!(pcm.mpeg1_extension().is_none());
        assert!(pcm.mpeg1_format.is_none());
    }

    #[test]
    fn pcm_constructors_set_mpeg1_format_to_none() {
        assert!(WaveFmt::new_pcm_mono(48000, 24).mpeg1_format.is_none());
        assert!(WaveFmt::new_pcm_stereo(48000, 16).mpeg1_format.is_none());
        assert!(WaveFmt::new_pcm_multichannel(48000, 24, 0x3F)
            .mpeg1_format
            .is_none());
        assert!(WaveFmt::new_pcm_ambisonic(48000, 24, 4)
            .mpeg1_format
            .is_none());
    }
}
