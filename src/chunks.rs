use std::io::{Read, Write};

use encoding::all::ASCII;
use encoding::Encoding;
use encoding::{DecoderTrap, EncoderTrap};

use byteorder::LittleEndian;
use byteorder::{ReadBytesExt, WriteBytesExt};

use uuid::Uuid;

use super::bext::Bext;
use super::errors::Error as ParserError;
use super::fact::Fact;
use super::fmt::{WaveFmt, WaveFmtExtended, WaveFmtMpeg1};
use super::mext::Mext;

pub trait ReadBWaveChunks: Read {
    fn read_bext(&mut self) -> Result<Bext, ParserError>;
    fn read_bext_string_field(&mut self, length: usize) -> Result<String, ParserError>;
    fn read_wave_fmt(&mut self) -> Result<WaveFmt, ParserError>;
    fn read_fact(&mut self) -> Result<Fact, ParserError>;
    fn read_mext(&mut self) -> Result<Mext, ParserError>;
}

pub trait WriteBWaveChunks: Write {
    fn write_wave_fmt(&mut self, format: &WaveFmt) -> Result<(), ParserError>;
    fn write_bext_string_field(&mut self, string: &str, length: usize) -> Result<(), ParserError>;
    fn write_bext(&mut self, bext: &Bext) -> Result<(), ParserError>;
    fn write_fact(&mut self, fact: &Fact) -> Result<(), ParserError>;
    fn write_mext(&mut self, mext: &Mext) -> Result<(), ParserError>;
}

impl<T> WriteBWaveChunks for T
where
    T: Write,
{
    fn write_wave_fmt(&mut self, format: &WaveFmt) -> Result<(), ParserError> {
        debug_assert!(
            !(format.extended_format.is_some() && format.mpeg1_format.is_some()),
            "WaveFmt has both extended_format and mpeg1_format set; these are mutually exclusive"
        );
        self.write_u16::<LittleEndian>(format.tag)?;
        self.write_u16::<LittleEndian>(format.channel_count)?;
        self.write_u32::<LittleEndian>(format.sample_rate)?;
        self.write_u32::<LittleEndian>(format.bytes_per_second)?;
        self.write_u16::<LittleEndian>(format.block_alignment)?;
        self.write_u16::<LittleEndian>(format.bits_per_sample)?;
        if let Some(ext) = format.extended_format {
            let cb_size = 24u16;
            self.write_u16::<LittleEndian>(cb_size)?;
            self.write_u16::<LittleEndian>(ext.valid_bits_per_sample)?;
            self.write_u32::<LittleEndian>(ext.channel_mask)?;
            let uuid = ext.type_guid.as_bytes();
            self.write_all(uuid)?;
        } else if let Some(mpeg1) = format.mpeg1_format {
            // MPEG1WAVEFORMAT: cbSize = 22 (8 fields totaling 22 bytes after cbSize).
            let cb_size = 22u16;
            self.write_u16::<LittleEndian>(cb_size)?;
            self.write_u16::<LittleEndian>(mpeg1.head_layer)?;
            self.write_u32::<LittleEndian>(mpeg1.head_bit_rate)?;
            self.write_u16::<LittleEndian>(mpeg1.head_mode)?;
            self.write_u16::<LittleEndian>(mpeg1.head_mode_ext)?;
            self.write_u16::<LittleEndian>(mpeg1.head_emphasis)?;
            self.write_u16::<LittleEndian>(mpeg1.head_flags)?;
            self.write_u32::<LittleEndian>(mpeg1.pts_low)?;
            self.write_u32::<LittleEndian>(mpeg1.pts_high)?;
        }
        Ok(())
    }

    fn write_bext_string_field(&mut self, string: &str, length: usize) -> Result<(), ParserError> {
        let mut buf = ASCII
            .encode(string, EncoderTrap::Ignore)
            .expect("Error encoding text");
        buf.truncate(length);
        let filler_length = length - buf.len();
        if filler_length > 0 {
            let mut filler = vec![0u8; filler_length];
            buf.append(&mut filler);
        }

        self.write_all(&buf)?;
        Ok(())
    }

    fn write_bext(&mut self, bext: &Bext) -> Result<(), ParserError> {
        self.write_bext_string_field(&bext.description, 256)?;
        self.write_bext_string_field(&bext.originator, 32)?;
        self.write_bext_string_field(&bext.originator_reference, 32)?;
        self.write_bext_string_field(&bext.origination_date, 10)?;
        self.write_bext_string_field(&bext.origination_time, 8)?;
        self.write_u64::<LittleEndian>(bext.time_reference)?;
        self.write_u16::<LittleEndian>(bext.version)?;

        let buf = bext.umid.unwrap_or([0u8; 64]);
        self.write_all(&buf)?;

        self.write_i16::<LittleEndian>((bext.loudness_value.unwrap_or(0.0) * 100.0) as i16)?;
        self.write_i16::<LittleEndian>((bext.loudness_range.unwrap_or(0.0) * 100.0) as i16)?;
        self.write_i16::<LittleEndian>((bext.max_true_peak_level.unwrap_or(0.0) * 100.0) as i16)?;
        self.write_i16::<LittleEndian>(
            (bext.max_momentary_loudness.unwrap_or(0.0) * 100.0) as i16,
        )?;
        self.write_i16::<LittleEndian>(
            (bext.max_short_term_loudness.unwrap_or(0.0) * 100.0) as i16,
        )?;

        let padding = [0u8; 180];
        self.write_all(&padding)?;

        let coding = ASCII
            .encode(&bext.coding_history, EncoderTrap::Ignore)
            .expect("Error");

        self.write_all(&coding)?;
        Ok(())
    }

    fn write_fact(&mut self, fact: &Fact) -> Result<(), ParserError> {
        self.write_u32::<LittleEndian>(fact.sample_length)?;
        Ok(())
    }

    fn write_mext(&mut self, mext: &Mext) -> Result<(), ParserError> {
        self.write_u16::<LittleEndian>(mext.sound_information)?;
        self.write_u16::<LittleEndian>(mext.frame_size)?;
        self.write_u16::<LittleEndian>(mext.ancillary_data_length)?;
        self.write_u16::<LittleEndian>(mext.ancillary_data_def)?;
        self.write_all(&mext.reserved)?;
        Ok(())
    }
}

impl<T> ReadBWaveChunks for T
where
    T: Read,
{
    fn read_wave_fmt(&mut self) -> Result<WaveFmt, ParserError> {
        let tag_value: u16;
        Ok(WaveFmt {
            tag: {
                tag_value = self.read_u16::<LittleEndian>()?;
                tag_value
            },
            channel_count: self.read_u16::<LittleEndian>()?,
            sample_rate: self.read_u32::<LittleEndian>()?,
            bytes_per_second: self.read_u32::<LittleEndian>()?,
            block_alignment: self.read_u16::<LittleEndian>()?,
            bits_per_sample: self.read_u16::<LittleEndian>()?,
            extended_format: {
                if tag_value == 0xFFFE {
                    let cb_size = self.read_u16::<LittleEndian>()?;
                    assert!(cb_size >= 22, "Format extension is not correct size");
                    Some(WaveFmtExtended {
                        valid_bits_per_sample: self.read_u16::<LittleEndian>()?,
                        channel_mask: self.read_u32::<LittleEndian>()?,
                        type_guid: {
                            let mut buf: [u8; 16] = [0; 16];
                            self.read_exact(&mut buf)?;
                            Uuid::from_slice(&buf)?
                        },
                    })
                } else {
                    None
                }
            },
            mpeg1_format: {
                // MPEG-1 audio (incl. MP2). EBU Tech 3285 Supplement 1, §2.1.
                if tag_value == 0x0050 {
                    let cb_size = self.read_u16::<LittleEndian>()?;
                    assert!(cb_size >= 22, "MPEG-1 fmt extension is not correct size");
                    Some(WaveFmtMpeg1 {
                        head_layer: self.read_u16::<LittleEndian>()?,
                        head_bit_rate: self.read_u32::<LittleEndian>()?,
                        head_mode: self.read_u16::<LittleEndian>()?,
                        head_mode_ext: self.read_u16::<LittleEndian>()?,
                        head_emphasis: self.read_u16::<LittleEndian>()?,
                        head_flags: self.read_u16::<LittleEndian>()?,
                        pts_low: self.read_u32::<LittleEndian>()?,
                        pts_high: self.read_u32::<LittleEndian>()?,
                    })
                } else {
                    None
                }
            },
        })
    }

    fn read_bext_string_field(&mut self, length: usize) -> Result<String, ParserError> {
        let mut buffer: Vec<u8> = vec![0; length];
        self.read_exact(&mut buffer)?;
        let trimmed: Vec<u8> = buffer.iter().take_while(|c| **c != 0_u8).cloned().collect();
        Ok(ASCII
            .decode(&trimmed, DecoderTrap::Ignore)
            .expect("Error decoding text"))
    }

    fn read_bext(&mut self) -> Result<Bext, ParserError> {
        let version: u16;
        Ok(Bext {
            description: self.read_bext_string_field(256)?,
            originator: self.read_bext_string_field(32)?,
            originator_reference: self.read_bext_string_field(32)?,
            origination_date: self.read_bext_string_field(10)?,
            origination_time: self.read_bext_string_field(8)?,
            time_reference: self.read_u64::<LittleEndian>()?,
            version: {
                version = self.read_u16::<LittleEndian>()?;
                version
            },
            umid: {
                let mut buf = [0u8; 64];
                self.read_exact(&mut buf)?;
                if version > 0 {
                    Some(buf)
                } else {
                    None
                }
            },
            loudness_value: {
                let val = (self.read_i16::<LittleEndian>()? as f32) / 100f32;
                if version > 1 {
                    Some(val)
                } else {
                    None
                }
            },
            loudness_range: {
                let val = self.read_i16::<LittleEndian>()? as f32 / 100f32;
                if version > 1 {
                    Some(val)
                } else {
                    None
                }
            },
            max_true_peak_level: {
                let val = self.read_i16::<LittleEndian>()? as f32 / 100f32;
                if version > 1 {
                    Some(val)
                } else {
                    None
                }
            },
            max_momentary_loudness: {
                let val = self.read_i16::<LittleEndian>()? as f32 / 100f32;
                if version > 1 {
                    Some(val)
                } else {
                    None
                }
            },
            max_short_term_loudness: {
                let val = self.read_i16::<LittleEndian>()? as f32 / 100f32;
                if version > 1 {
                    Some(val)
                } else {
                    None
                }
            },
            coding_history: {
                for _ in 0..180 {
                    self.read_u8()?;
                }
                let mut buf = vec![];
                self.read_to_end(&mut buf)?;
                ASCII
                    .decode(&buf, DecoderTrap::Ignore)
                    .expect("Error decoding text")
            },
        })
    }

    fn read_fact(&mut self) -> Result<Fact, ParserError> {
        Ok(Fact {
            sample_length: self.read_u32::<LittleEndian>()?,
        })
    }

    fn read_mext(&mut self) -> Result<Mext, ParserError> {
        Ok(Mext {
            sound_information: self.read_u16::<LittleEndian>()?,
            frame_size: self.read_u16::<LittleEndian>()?,
            ancillary_data_length: self.read_u16::<LittleEndian>()?,
            ancillary_data_def: self.read_u16::<LittleEndian>()?,
            reserved: {
                let mut buf = [0u8; 4];
                self.read_exact(&mut buf)?;
                buf
            },
        })
    }
}

#[test]
fn test_read_51_wav() {
    use super::common_format::CommonFormat;
    use super::fmt::ChannelMask;

    let path = "tests/media/pt_24bit_51.wav";

    let mut w = super::wavereader::WaveReader::open(path).unwrap();
    let format = w.format().unwrap();
    assert_eq!(format.tag, 0xFFFE);
    assert_eq!(format.channel_count, 6);
    assert_eq!(format.sample_rate, 48000);
    let extended = format.extended_format.unwrap();

    assert_eq!(extended.valid_bits_per_sample, 24);

    let channels = ChannelMask::channels(extended.channel_mask, format.channel_count);

    assert_eq!(
        channels,
        [
            ChannelMask::FrontLeft,
            ChannelMask::FrontRight,
            ChannelMask::FrontCenter,
            ChannelMask::LowFrequency,
            ChannelMask::BackLeft,
            ChannelMask::BackRight
        ]
    );

    assert_eq!(format.common_format(), CommonFormat::IntegerPCM);
}
