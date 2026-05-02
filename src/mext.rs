/// `mext` chunk record — MPEG audio extension for Broadcast Wave Files.
///
/// Defined by [EBU Tech 3285 Supplement 1][s1] for files containing MPEG
/// audio in the `data` chunk. Carries information about the framing of
/// the MPEG bitstream that cannot be expressed in the standard `fmt`
/// chunk: whether all frames are homogeneous, padding-bit usage,
/// sample-rate locking, free-format detection, and ancillary-data
/// region size.
///
/// Total chunk content size is exactly **12 bytes** (plus 8 bytes of
/// chunk header in the WAVE container).
///
/// All fields are little-endian, despite the MPEG bitstream itself
/// being big-endian — the EBU specification places `mext` in the
/// surrounding RIFF byte order.
///
/// ## Resources
/// - [EBU Tech 3285 Supplement 1][s1]
///
/// [s1]: https://tech.ebu.ch/docs/tech/tech3285s1.pdf
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Mext {
    /// Bitfield describing the MPEG bitstream's framing properties.
    /// See the `SoundInformation` flag constants in this module.
    pub sound_information: u16,

    /// Size in bytes of an MPEG audio frame, when the bitstream is
    /// homogeneous (all frames the same size).
    ///
    /// Should equal the WAVE `fmt` chunk's `block_alignment` for
    /// homogeneous streams.
    pub frame_size: u16,

    /// Length in bytes of the ancillary data region within each MPEG
    /// frame, if any (0 if absent or unknown).
    pub ancillary_data_length: u16,

    /// Bitfield describing the type/usage of ancillary data, per
    /// EBU Tech 3285 Supplement 1.
    pub ancillary_data_def: u16,

    /// Reserved 4 bytes; must round-trip verbatim for forward
    /// compatibility with future EBU additions.
    pub reserved: [u8; 4],
}

/// `sound_information` bitfield flags (EBU Tech 3285 Supplement 1, §2.2).
impl Mext {
    /// Bit 0: all frames in the file are homogeneous (same frame
    /// size, sample rate, channel mode, etc.).
    pub const SOUND_HOMOGENEOUS: u16 = 0x0001;

    /// Bit 1: the MPEG padding bit is never set in this file.
    /// Only meaningful when [`SOUND_HOMOGENEOUS`](Self::SOUND_HOMOGENEOUS) is set.
    pub const SOUND_PADDING_BIT_UNUSED: u16 = 0x0002;

    /// Bit 2: file contains some frames with padding (e.g. the 44.1 kHz
    /// or 22.05 kHz family where padding is used to keep the average
    /// bitrate exact).
    pub const SOUND_PADDING_BIT_USED: u16 = 0x0004;

    /// Bit 3: file uses MPEG free format (no fixed bitrate; frame size
    /// is signaled outside the bitstream).
    pub const SOUND_FREE_FORMAT: u16 = 0x0008;

    /// Build an `Mext` from parsed MPEG frame info.
    ///
    /// Sets the `SOUND_HOMOGENEOUS` bit if the input reports stable
    /// frame size across all frames, the `SOUND_FREE_FORMAT` bit if
    /// the bitstream uses free format, and the appropriate padding
    /// bit based on the first frame's padding flag (the
    /// `PADDING_BIT_USED` bit fires for the 44.1/22.05 kHz family
    /// where padding is expected). `frame_size` is taken from the
    /// first frame.
    pub fn from_mpeg_info(info: &crate::mpeg::MpegInfo) -> Self {
        let mut sound_information: u16 = 0;
        if info.homogeneous {
            sound_information |= Self::SOUND_HOMOGENEOUS;
        }
        if !info.padding {
            sound_information |= Self::SOUND_PADDING_BIT_UNUSED;
        }
        // Set the 44.1/22.05 kHz "padding may be used" bit only when
        // the sample rate is from that family AND the first frame has
        // padding. This matches the JS reference implementation's
        // mpegSoundInformation_ semantics.
        let in_44_or_22_family = info.sample_rate == 44100 || info.sample_rate == 22050;
        if in_44_or_22_family && info.padding {
            sound_information |= Self::SOUND_PADDING_BIT_USED;
        }
        if info.free_format {
            sound_information |= Self::SOUND_FREE_FORMAT;
        }
        Mext {
            sound_information,
            frame_size: info.frame_size as u16,
            ancillary_data_length: 0,
            ancillary_data_def: 0,
            reserved: [0; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{ReadBWaveChunks, WriteBWaveChunks};
    use crate::wavereader::WaveReader;
    use crate::wavewriter::WaveWriter;
    use crate::WaveFmt;
    use std::io::{Cursor, Seek, SeekFrom};

    fn round_trip(mext: Mext) {
        let mut buf = Cursor::new(Vec::<u8>::new());
        buf.write_mext(&mext).unwrap();
        assert_eq!(buf.get_ref().len(), 12, "mext content must be 12 bytes");
        buf.seek(SeekFrom::Start(0)).unwrap();
        let parsed = buf.read_mext().unwrap();
        assert_eq!(parsed, mext);
    }

    #[test]
    fn round_trip_zeros() {
        round_trip(Mext {
            sound_information: 0,
            frame_size: 0,
            ancillary_data_length: 0,
            ancillary_data_def: 0,
            reserved: [0; 4],
        });
    }

    #[test]
    fn round_trip_all_sound_information_combinations() {
        // All 16 combinations of the 4 defined sound_information bits.
        for bits in 0u16..16 {
            round_trip(Mext {
                sound_information: bits,
                frame_size: 144,
                ancillary_data_length: 0,
                ancillary_data_def: 0,
                reserved: [0; 4],
            });
        }
    }

    #[test]
    fn round_trip_max_values() {
        round_trip(Mext {
            sound_information: u16::MAX,
            frame_size: u16::MAX,
            ancillary_data_length: u16::MAX,
            ancillary_data_def: u16::MAX,
            reserved: [0xFF; 4],
        });
    }

    #[test]
    fn reserved_bytes_round_trip_verbatim() {
        round_trip(Mext {
            sound_information: 0,
            frame_size: 0,
            ancillary_data_length: 0,
            ancillary_data_def: 0,
            reserved: [0xDE, 0xAD, 0xBE, 0xEF],
        });
    }

    #[test]
    fn round_trip_via_wave_writer() {
        let mext = Mext {
            sound_information: Mext::SOUND_HOMOGENEOUS | Mext::SOUND_PADDING_BIT_UNUSED,
            frame_size: 144,
            ancillary_data_length: 0,
            ancillary_data_def: 0,
            reserved: [0; 4],
        };

        let mut cursor = Cursor::new(Vec::<u8>::new());
        let format = WaveFmt::new_pcm_mono(48000, 24);
        let mut w = WaveWriter::new(&mut cursor, format).unwrap();
        w.write_mext(&mext).unwrap();
        // Append a minimal data chunk so the file passes WaveReader's
        // fmt-and-data validation.
        let mut frame_writer = w.audio_frame_writer().unwrap();
        frame_writer.write_frames(&[0i32]).unwrap();
        frame_writer.end().unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(cursor).unwrap();
        let parsed = reader
            .mext()
            .unwrap()
            .expect("mext chunk should be present");
        assert_eq!(parsed, mext);
    }

    #[test]
    fn no_mext_chunk_returns_none() {
        // pt_24bit.wav is plain PCM with no mext chunk.
        let mut reader = WaveReader::open("tests/media/pt_24bit.wav").unwrap();
        assert!(reader.mext().unwrap().is_none());
    }

    #[test]
    fn read_60315_wav_has_mext() {
        // 60315.wav is a real broadcast WAV with MP2 audio; mext is mandatory.
        let mut reader = WaveReader::open("tests/media/60315.wav").unwrap();
        let mext = reader
            .mext()
            .unwrap()
            .expect("60315.wav must have an mext chunk");
        // Frame size for MPEG-1 Layer II at typical broadcast bitrates is
        // non-zero; sound_information should at least have the homogeneous
        // bit set for a CBR broadcast file.
        assert!(mext.frame_size > 0);
        assert!(mext.sound_information & Mext::SOUND_HOMOGENEOUS != 0);
    }
}
