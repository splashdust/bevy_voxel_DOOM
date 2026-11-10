//! DOOM's sound effects: the DMX format of the `DS` lumps.
//!
//! A lump is a u16 format (3), a u16 sample rate, a u32 sample count, then
//! that many 8-bit unsigned PCM samples, of which the first and last 16 are
//! padding.

use crate::bytes::{u16_at, u32_at};
use crate::{Wad, WadError};

/// Padding samples at each end of a DMX sound.
const PAD: usize = 16;

/// A mono sound of 8-bit unsigned samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    pub rate: u16,
    pub samples: Vec<u8>,
}

impl Sound {
    /// Decodes a DMX sound lump.
    pub fn decode(data: &[u8]) -> Result<Sound, WadError> {
        let malformed = |m: String| WadError::Malformed(format!("sound: {m}"));
        let format = u16_at(data, 0).ok_or_else(|| malformed("no header".into()))?;
        if format != 3 {
            return Err(malformed(format!("format {format}, not 3")));
        }
        let rate = u16_at(data, 2).ok_or_else(|| malformed("no rate".into()))?;
        let count =
            u32_at(data, 4).ok_or_else(|| malformed("no length".into()))? as usize;
        let samples = data.get(8..8 + count).ok_or_else(|| {
            malformed(format!(
                "{count} samples, {} bytes",
                data.len().saturating_sub(8)
            ))
        })?;
        // Some lumps lack the padding; keep what is there.
        let samples = if count > 2 * PAD {
            &samples[PAD..count - PAD]
        } else {
            samples
        };
        Ok(Sound {
            rate,
            samples: samples.to_vec(),
        })
    }

    /// The sound as a WAV file: 8-bit unsigned mono PCM.
    pub fn to_wav(&self) -> Vec<u8> {
        let len = self.samples.len() as u32;
        let rate = self.rate as u32;
        let mut wav = Vec::with_capacity(44 + self.samples.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        // PCM, one channel, the rate, bytes a second, block align, bits.
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&len.to_le_bytes());
        wav.extend_from_slice(&self.samples);
        wav
    }
}

impl Wad {
    /// The sound lump `DS` + `name`.
    pub fn sound(&self, name: &str) -> Result<Sound, WadError> {
        let lump_name = format!("DS{}", name.to_ascii_uppercase());
        let lump = self
            .lump(&lump_name)
            .ok_or_else(|| WadError::MissingLump(lump_name.clone()))?;
        Sound::decode(lump).map_err(|e| match e {
            WadError::Malformed(m) => WadError::Malformed(format!("{lump_name}: {m}")),
            e => e,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::build_wad;

    fn dmx(rate: u16, body: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&3u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&((body.len() + 2 * PAD) as u32).to_le_bytes());
        d.extend_from_slice(&[128; PAD]);
        d.extend_from_slice(body);
        d.extend_from_slice(&[128; PAD]);
        d
    }

    #[test]
    fn decodes_dmx_and_drops_the_padding() {
        let wad =
            Wad::from_bytes(build_wad(&[("DSPISTOL", &dmx(11025, &[1, 2, 3]))])).unwrap();
        let sound = wad.sound("pistol").unwrap();
        assert_eq!(sound.rate, 11025);
        assert_eq!(sound.samples, [1, 2, 3]);
        assert!(matches!(wad.sound("NOPE"), Err(WadError::MissingLump(_))));
        let mut bad = dmx(11025, &[1]);
        bad[0] = 2;
        assert!(Sound::decode(&bad).is_err());
        let short = dmx(11025, &[1, 2, 3]);
        assert!(Sound::decode(&short[..short.len() - 1]).is_err());
    }

    #[test]
    fn wav_header_describes_the_samples() {
        let wav = Sound {
            rate: 11025,
            samples: vec![10, 20, 30],
        }
        .to_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 39);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 11025);
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 8);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[44..], [10, 20, 30]);
    }
}
