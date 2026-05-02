use std::io::{Read, Seek, SeekFrom};

use mp3_metadata::{
    ChannelType as MmChannelType, Emphasis as MmEmphasis, Layer as MmLayer, Version as MmVersion,
};

use super::errors::Error;

/// MPEG audio version, decoded from the frame header.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MpegVersion {
    /// MPEG-1 (sample rates: 32 / 44.1 / 48 kHz).
    V1,
    /// MPEG-2 (sample rates: 16 / 22.05 / 24 kHz).
    V2,
    /// MPEG-2.5 unofficial extension (sample rates: 8 / 11.025 / 12 kHz).
    V25,
}

/// MPEG audio layer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MpegLayer {
    /// Layer I (384 samples per frame).
    Layer1,
    /// Layer II — used for broadcast distribution (1152 samples per frame). Most relevant for MP2.
    Layer2,
    /// Layer III (MP3) — 1152 samples for MPEG-1, 576 for MPEG-2/2.5.
    Layer3,
}

/// MPEG audio channel mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChannelMode {
    Stereo,
    JointStereo,
    DualMono,
    Mono,
}

/// Parsed MPEG audio frame information, populated from the file's first
/// MPEG frame plus an aggregate scan to compute total length and
/// homogeneity.
///
/// This struct is the bridge between an arbitrary MPEG bitstream and
/// the WAVE chunks that describe it (`fmt` extended for MPEG, `mext`,
/// `fact`, `bext` coding history). The actual frame-header parsing is
/// delegated to the [`mp3-metadata`] crate; this struct exposes the
/// derived and aggregate fields needed for BWF wrapper generation.
///
/// All "first frame" fields (`bit_rate`, `sample_rate`, `padding`,
/// etc.) describe frame zero. For homogeneous (CBR) bitstreams these
/// hold for every frame; for VBR streams they hold for the first frame
/// only and `homogeneous` will be `false`.
///
/// [`mp3-metadata`]: https://crates.io/crates/mp3-metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpegInfo {
    /// MPEG version (1, 2, or 2.5).
    pub version: MpegVersion,

    /// MPEG layer (I, II, III).
    pub layer: MpegLayer,

    /// Bit rate of frame zero, in **kilobits per second**.
    /// Zero indicates free-format (no fixed bitrate); see [`free_format`](Self::free_format).
    pub bit_rate: u32,

    /// Sample rate in Hz.
    pub sample_rate: u32,

    /// Channel mode of frame zero.
    pub channel_mode: ChannelMode,

    /// Channel count derived from `channel_mode` (1 for mono, 2 otherwise).
    pub num_channels: u16,

    /// Raw 2-bit `mode_extension` field from the MPEG header
    /// (`(ms_stereo << 1) | intensity_stereo`). Interpretation is
    /// **layer-dependent**: for Layer III these are bit flags, for
    /// Layer I/II they're a band-range index (0..=3). Only meaningful
    /// when `channel_mode` is [`JointStereo`](ChannelMode::JointStereo).
    pub mode_extension: u8,

    /// Samples per frame, derived from `(version, layer)`:
    /// 384 for Layer I, 1152 for Layer II, 1152 for MPEG-1 Layer III,
    /// 576 for MPEG-2/2.5 Layer III.
    pub samples_per_frame: u32,

    /// Frame zero's frame size in bytes (header + audio data + optional CRC).
    pub frame_size: u32,

    /// Total number of samples per channel after decoding, computed as
    /// `frame_count * samples_per_frame`. Suitable for the `fact` chunk
    /// and the bext coding history's sample-count math.
    pub sample_length: u64,

    /// Padding bit of frame zero.
    pub padding: bool,

    /// Private bit of frame zero (semantics vendor-defined).
    pub private_bit: bool,

    /// Copyright bit of frame zero.
    pub copyright: bool,

    /// Original-or-copy bit of frame zero (`true` = original).
    pub original: bool,

    /// Whether frame zero declares CRC error protection.
    pub error_protection: bool,

    /// Raw 2-bit emphasis field from the MPEG header
    /// (0 = none, 1 = 50/15µs, 2 = reserved, 3 = CCIT J.17).
    pub emphasis: u8,

    /// Byte offset of the first MPEG frame within the source data.
    /// Non-zero when an ID3v2 tag prefixes the audio; the size of the
    /// ID3v2 region equals this value.
    pub id3v2_offset: u64,

    /// True if every parsed frame has the same bitrate, sample rate,
    /// and channel mode. CBR broadcast files are homogeneous; VBR is
    /// not. Drives the `mext.sound_information` `HOMOGENEOUS` bit.
    pub homogeneous: bool,

    /// True if frame zero declares free format (bitrate index 0).
    pub free_format: bool,
}

impl MpegInfo {
    /// Parse MPEG audio frame headers from a byte buffer.
    ///
    /// The buffer may begin with an ID3v2 tag; that prefix is detected
    /// and skipped (its size is reported in [`id3v2_offset`](Self::id3v2_offset)).
    /// Returns an error if no valid MPEG frames are found.
    pub fn from_buffer(buf: &[u8]) -> Result<Self, Error> {
        let meta = mp3_metadata::read_from_slice(buf)
            .map_err(|e| Error::MpegParseError(format!("{}", e)))?;

        let f0 = meta
            .frames
            .first()
            .ok_or_else(|| Error::MpegParseError("no MPEG frames found".into()))?;

        let version = match f0.version {
            MmVersion::MPEG1 => MpegVersion::V1,
            MmVersion::MPEG2 => MpegVersion::V2,
            MmVersion::MPEG2_5 => MpegVersion::V25,
            other => {
                return Err(Error::MpegParseError(format!(
                    "unsupported MPEG version: {:?}",
                    other
                )))
            }
        };

        let layer = match f0.layer {
            MmLayer::Layer1 => MpegLayer::Layer1,
            MmLayer::Layer2 => MpegLayer::Layer2,
            MmLayer::Layer3 => MpegLayer::Layer3,
            other => {
                return Err(Error::MpegParseError(format!(
                    "unsupported MPEG layer: {:?}",
                    other
                )))
            }
        };

        let channel_mode = match f0.chan_type {
            MmChannelType::Stereo => ChannelMode::Stereo,
            MmChannelType::JointStereo => ChannelMode::JointStereo,
            MmChannelType::DualChannel => ChannelMode::DualMono,
            MmChannelType::SingleChannel => ChannelMode::Mono,
            MmChannelType::Unknown => {
                return Err(Error::MpegParseError("unknown channel mode".into()));
            }
        };

        let num_channels = if channel_mode == ChannelMode::Mono {
            1
        } else {
            2
        };

        let samples_per_frame: u32 = match (version, layer) {
            (_, MpegLayer::Layer1) => 384,
            (_, MpegLayer::Layer2) => 1152,
            (MpegVersion::V1, MpegLayer::Layer3) => 1152,
            (_, MpegLayer::Layer3) => 576,
        };

        let sample_length = (meta.frames.len() as u64) * samples_per_frame as u64;

        // Per EBU Tech 3285 S1, "homogeneous" means stable frame size
        // (bit rate + sample rate + channel count). The raw channel-mode
        // bit can legally flip between Stereo / JointStereo / DualMono
        // per-frame within a CBR 2-channel stream — this is real-world
        // MP2 behavior and should not flag the file as VBR.
        fn channel_count(c: MmChannelType) -> u8 {
            match c {
                MmChannelType::SingleChannel => 1,
                _ => 2,
            }
        }
        let f0_channels = channel_count(f0.chan_type);
        let homogeneous = meta.frames.iter().all(|f| {
            f.bitrate == f0.bitrate
                && f.sampling_freq == f0.sampling_freq
                && channel_count(f.chan_type) == f0_channels
        });

        // mp3-metadata stores the 2-bit mode_extension field as two bools:
        //   bit 0 (LSB, intensity_stereo) and bit 1 (MSB, ms_stereo).
        // Reconstruct the raw 2-bit value:
        let mode_extension = ((f0.ms_stereo as u8) << 1) | (f0.intensity_stereo as u8);

        let copyright = matches!(f0.copyright, mp3_metadata::Copyright::Some);
        let original = matches!(f0.status, mp3_metadata::Status::Original);
        let error_protection = matches!(f0.crc, mp3_metadata::CRC::Added);

        let emphasis: u8 = match f0.emphasis {
            MmEmphasis::None => 0,
            MmEmphasis::MicroSeconds => 1,
            MmEmphasis::Reserved => 2,
            MmEmphasis::CCITT => 3,
            MmEmphasis::Unknown => 0,
        };

        Ok(MpegInfo {
            version,
            layer,
            bit_rate: f0.bitrate as u32,
            sample_rate: f0.sampling_freq as u32,
            channel_mode,
            num_channels,
            mode_extension,
            samples_per_frame,
            frame_size: f0.size,
            sample_length,
            padding: f0.padding,
            private_bit: f0.private_bit,
            copyright,
            original,
            error_protection,
            emphasis,
            id3v2_offset: f0.offset as u64,
            homogeneous,
            free_format: f0.bitrate == 0,
        })
    }

    /// Parse MPEG audio frame headers from a `Read + Seek` source. The
    /// reader is rewound to position 0 before reading.
    ///
    /// The reader is fully buffered into memory before parsing — fine
    /// for typical MP2 broadcast files (single-digit MB), unsuitable
    /// for streams.
    pub fn from_reader<R: Read + Seek>(reader: &mut R) -> Result<Self, Error> {
        reader.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        Self::from_buffer(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_fixture(name: &str) -> Vec<u8> {
        fs::read(format!("tests/media/{}", name))
            .expect("fixture missing — run create_test_media.sh")
    }

    #[test]
    fn parse_test_mp2() {
        let buf = read_fixture("test.mp2");
        let info = MpegInfo::from_buffer(&buf).unwrap();
        assert_eq!(info.version, MpegVersion::V1);
        assert_eq!(info.layer, MpegLayer::Layer2);
        assert_eq!(info.samples_per_frame, 1152);
        assert!(
            info.bit_rate >= 32,
            "expected real bitrate, got {}",
            info.bit_rate
        );
        assert!(info.sample_rate > 0);
        assert_eq!(info.id3v2_offset, 0, "test.mp2 has no ID3v2 prefix");
        assert!(info.frame_size > 0);
        assert!(info.sample_length > 0);
        assert!(info.homogeneous, "test.mp2 should be CBR");
        assert!(!info.free_format);
    }

    #[test]
    fn parse_test_id3_mp2() {
        let buf = read_fixture("test-id3.mp2");
        let info = MpegInfo::from_buffer(&buf).unwrap();
        assert!(
            info.id3v2_offset > 0,
            "test-id3.mp2 should have an ID3v2 prefix; got offset {}",
            info.id3v2_offset
        );
        // The audio body matches test.mp2: same layer, version, samples_per_frame.
        let plain = MpegInfo::from_buffer(&read_fixture("test.mp2")).unwrap();
        assert_eq!(info.version, plain.version);
        assert_eq!(info.layer, plain.layer);
        assert_eq!(info.samples_per_frame, plain.samples_per_frame);
        assert_eq!(info.bit_rate, plain.bit_rate);
        assert_eq!(info.sample_rate, plain.sample_rate);
        assert_eq!(info.channel_mode, plain.channel_mode);
    }

    #[test]
    fn parse_test_bad_mp2() {
        let buf = read_fixture("test-bad.mp2");
        let result = MpegInfo::from_buffer(&buf);
        assert!(
            result.is_err() || matches!(&result, Ok(info) if !info.homogeneous),
            "test-bad.mp2 should fail to parse or be non-homogeneous; got {:?}",
            result.as_ref().ok()
        );
    }

    #[test]
    fn parse_60315_wav_mpeg_data_chunk() {
        // 60315.wav embeds an MP2 stream in its data chunk. Use the
        // crate's RIFF parser to locate the data chunk, then parse the
        // embedded MP2 frames.
        use crate::fourcc::DATA_SIG;
        use crate::parser::Parser;
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};

        let mut f = File::open("tests/media/60315.wav").unwrap();
        let chunks = Parser::make(&mut f).unwrap().into_chunk_list().unwrap();
        let data_chunk = chunks
            .iter()
            .find(|c| c.signature == DATA_SIG)
            .expect("60315.wav must have a data chunk");

        let mut mp2 = vec![0u8; data_chunk.length as usize];
        f.seek(SeekFrom::Start(data_chunk.start)).unwrap();
        f.read_exact(&mut mp2).unwrap();

        let info = MpegInfo::from_buffer(&mp2).unwrap();
        assert_eq!(info.layer, MpegLayer::Layer2);
        assert_eq!(info.samples_per_frame, 1152);
        assert!(info.homogeneous, "60315.wav broadcast MP2 should be CBR");
    }

    #[test]
    fn from_reader_matches_from_buffer() {
        use std::io::Cursor;
        let buf = read_fixture("test.mp2");
        let from_buf = MpegInfo::from_buffer(&buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let from_rdr = MpegInfo::from_reader(&mut cursor).unwrap();
        assert_eq!(from_buf, from_rdr);
    }
}
