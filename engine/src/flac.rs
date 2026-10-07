//! Streaming FLAC encoder for stereo 16 and 24-bit exports. One block of samples is held at a
//! time: each is coded with the best fixed predictor and partitioned Rice parameters, in the
//! cheaper of left/right and mid/side, and written before the next is rendered. STREAMINFO
//! (length and MD5 of the audio) is patched in when the stream ends.
use crate::Result;
use std::io::{Seek, SeekFrom, Write};

const BLOCK: usize = 4096;
const MAX_PARTITION_ORDER: u32 = 6;
/// Rice2 parameters are five bits; 31 is the escape code, which this encoder never needs.
const MAX_RICE: u32 = 30;

pub struct Encoder<W: Write + Seek> {
    out: W,
    rate: u32,
    bits: u32,
    /// Interleaved samples of the block being filled.
    pending: Vec<i32>,
    frame: BitWriter,
    frame_number: u64,
    frames: u64,
    md5: Md5,
    smallest: usize,
    largest: usize,
    scratch: Scratch,
}
#[derive(Default)]
struct Scratch {
    /// Left, right, mid, side.
    channels: [Vec<i64>; 4],
    residual: Vec<i64>,
}

impl<W: Write + Seek> Encoder<W> {
    pub fn new(mut out: W, rate: u32, bits: u32) -> Result<Self> {
        if bits != 16 && bits != 24 {
            return Err(
                "FLAC holds 16 or 24-bit audio here; choose pcm16 or pcm24, or export float32 as WAV"
                    .into(),
            );
        }
        if !(1..=655_350).contains(&rate) {
            return Err("FLAC cannot hold this sample rate".into());
        }
        out.write_all(b"fLaC").map_err(|e| e.to_string())?;
        // Placeholder STREAMINFO, rewritten by `finish`.
        out.write_all(&[0x80, 0, 0, 34])
            .map_err(|e| e.to_string())?;
        out.write_all(&[0; 34]).map_err(|e| e.to_string())?;
        Ok(Self {
            out,
            rate,
            bits,
            pending: Vec::with_capacity(BLOCK * 2),
            frame: BitWriter::default(),
            frame_number: 0,
            frames: 0,
            md5: Md5::new(),
            smallest: usize::MAX,
            largest: 0,
            scratch: Scratch::default(),
        })
    }
    /// One sample; left then right, as the exporter produces them.
    pub fn write(&mut self, sample: i32) -> Result<()> {
        self.pending.push(sample);
        if self.pending.len() == BLOCK * 2 {
            self.flush_block()?;
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<()> {
        if !self.pending.len().is_multiple_of(2) {
            return Err("FLAC export ended in the middle of a stereo frame".into());
        }
        if !self.pending.is_empty() {
            self.flush_block()?;
        }
        if self.frames >= 1 << 36 {
            return Err("This export is too long for one FLAC file".into());
        }
        let mut info = BitWriter::default();
        info.put(BLOCK as u64, 16);
        info.put(BLOCK as u64, 16);
        info.put(self.smallest.min(self.largest) as u64 & 0xff_ffff, 24);
        info.put(self.largest as u64 & 0xff_ffff, 24);
        info.put(self.rate as u64, 20);
        info.put(1, 3);
        info.put(self.bits as u64 - 1, 5);
        info.put(self.frames, 36);
        let mut block = info.bytes;
        block.extend_from_slice(&self.md5.finish());
        self.out.flush().map_err(|e| e.to_string())?;
        self.out
            .seek(SeekFrom::Start(8))
            .map_err(|e| e.to_string())?;
        self.out.write_all(&block).map_err(|e| e.to_string())?;
        self.out.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        self.out.flush().map_err(|e| e.to_string())
    }

    fn flush_block(&mut self) -> Result<()> {
        let count = self.pending.len() / 2;
        let bytes = self.bits as usize / 8;
        for sample in &self.pending {
            self.md5.update(&sample.to_le_bytes()[..bytes]);
        }
        let Scratch { channels, residual } = &mut self.scratch;
        for channel in channels.iter_mut() {
            channel.clear();
        }
        for pair in self.pending.as_chunks::<2>().0 {
            let (left, right) = (pair[0] as i64, pair[1] as i64);
            channels[0].push(left);
            channels[1].push(right);
            channels[2].push((left + right) >> 1);
            channels[3].push(left - right);
        }
        let plans: [Plan; 4] = std::array::from_fn(|index| {
            let bits = self.bits + u32::from(index == 3);
            plan(&channels[index], bits, residual)
        });
        // Left/right against mid/side: whichever pair of subframes is smaller.
        let (assignment, first, second, second_bits) =
            if plans[2].bits + plans[3].bits < plans[0].bits + plans[1].bits {
                (0b1010, 2, 3, self.bits + 1)
            } else {
                (0b0001, 0, 1, self.bits)
            };

        let frame = &mut self.frame;
        frame.clear();
        frame.put(0x3ffe, 14); // sync code 11111111111110
        frame.put(0, 2);
        let size_code = match count {
            256 => 0b1000,
            512 => 0b1001,
            1024 => 0b1010,
            2048 => 0b1011,
            4096 => 0b1100,
            n if n <= 256 => 0b0110,
            _ => 0b0111,
        };
        frame.put(size_code, 4);
        let rate_code = match self.rate {
            44100 => 0b1001,
            48000 => 0b1010,
            96000 => 0b1011,
            _ => 0,
        };
        frame.put(rate_code, 4);
        frame.put(assignment, 4);
        frame.put(if self.bits == 16 { 0b100 } else { 0b110 }, 3);
        frame.put(0, 1);
        frame.put_utf8(self.frame_number);
        match size_code {
            0b0110 => frame.put(count as u64 - 1, 8),
            0b0111 => frame.put(count as u64 - 1, 16),
            _ => {}
        }
        let header_crc = crc8(&frame.bytes);
        frame.put(header_crc as u64, 8);
        subframe(frame, &channels[first], self.bits, plans[first], residual);
        subframe(
            frame,
            &channels[second],
            second_bits,
            plans[second],
            residual,
        );
        frame.align();
        let crc = crc16(&frame.bytes);
        frame.put(crc as u64, 16);
        self.out
            .write_all(&frame.bytes)
            .map_err(|e| e.to_string())?;
        self.smallest = self.smallest.min(frame.bytes.len());
        self.largest = self.largest.max(frame.bytes.len());
        self.frame_number += 1;
        self.frames += count as u64;
        self.pending.clear();
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Constant,
    Verbatim,
    Fixed(usize),
}
#[derive(Clone, Copy)]
struct Plan {
    kind: Kind,
    bits: u64,
    partition_order: u32,
}
const COEFFICIENTS: [&[i64]; 5] = [&[], &[1], &[2, -1], &[3, -3, 1], &[4, -6, 4, -1]];

fn residual_at(samples: &[i64], order: usize, n: usize) -> i64 {
    let mut prediction = 0;
    for (k, c) in COEFFICIENTS[order].iter().enumerate() {
        prediction += c * samples[n - 1 - k];
    }
    samples[n] - prediction
}
fn residuals(samples: &[i64], order: usize, out: &mut Vec<i64>) {
    out.clear();
    out.extend((order..samples.len()).map(|n| residual_at(samples, order, n)));
}
fn fold(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}
/// The Rice parameter for `count` folded residuals that add up to `sum`, and about how many
/// bits they take with it.
fn rice(sum: u64, count: usize) -> (u32, u64) {
    let mean = sum / count.max(1) as u64;
    let k = (64 - mean.leading_zeros()).saturating_sub(1).min(MAX_RICE);
    (k, count as u64 * (k as u64 + 1) + (sum >> k))
}
fn partition_length(count: usize, order: usize, partition_order: u32, index: usize) -> usize {
    (count >> partition_order) - if index == 0 { order } else { 0 }
}
/// The partition order that needs the fewest bits, and that size. Sums are taken once at
/// the finest split and merged upwards.
fn partition(residual: &[i64], order: usize, count: usize) -> (u32, u64) {
    let mut finest = 0;
    while finest < MAX_PARTITION_ORDER
        && count.is_multiple_of(2 << finest)
        && count / (2 << finest) > order
    {
        finest += 1;
    }
    let mut sums = [0u64; 1 << MAX_PARTITION_ORDER];
    let mut at = 0;
    for (index, sum) in sums[..1 << finest].iter_mut().enumerate() {
        let length = partition_length(count, order, finest, index);
        *sum = residual[at..at + length].iter().map(|r| fold(*r)).sum();
        at += length;
    }
    let mut best = (0, u64::MAX);
    for p in (0..=finest).rev() {
        let bits: u64 = (0..1usize << p)
            .map(|index| 5 + rice(sums[index], partition_length(count, order, p, index)).1)
            .sum();
        if bits < best.1 {
            best = (p, bits);
        }
        for index in 0..(1usize << p) / 2 {
            sums[index] = sums[2 * index] + sums[2 * index + 1];
        }
    }
    best
}
fn plan(samples: &[i64], bits: u32, residual: &mut Vec<i64>) -> Plan {
    let count = samples.len();
    if samples.iter().all(|s| *s == samples[0]) {
        return Plan {
            kind: Kind::Constant,
            bits: 8 + bits as u64,
            partition_order: 0,
        };
    }
    let verbatim = Plan {
        kind: Kind::Verbatim,
        bits: 8 + bits as u64 * count as u64,
        partition_order: 0,
    };
    // The predictor with the smallest residual, judged where all five can be compared.
    let first = 4.min(count - 1);
    let order = (0..=first)
        .min_by_key(|order| {
            (first..count)
                .map(|n| fold(residual_at(samples, *order, n)))
                .fold(0u64, u64::saturating_add)
        })
        .unwrap_or(0);
    residuals(samples, order, residual);
    // A residual must fit 32 bits; a block that wild is stored as it is.
    if residual.iter().any(|r| r.unsigned_abs() >= 1 << 31) {
        return verbatim;
    }
    let (partition_order, coded) = partition(residual, order, count);
    let size = 8 + bits as u64 * order as u64 + 6 + coded;
    if size < verbatim.bits {
        Plan {
            kind: Kind::Fixed(order),
            bits: size,
            partition_order,
        }
    } else {
        verbatim
    }
}
fn subframe(out: &mut BitWriter, samples: &[i64], bits: u32, plan: Plan, residual: &mut Vec<i64>) {
    let count = samples.len();
    let signed =
        |out: &mut BitWriter, value: i64| out.put(value as u64 & ((1u64 << bits) - 1), bits);
    match plan.kind {
        Kind::Constant => {
            out.put(0, 8);
            signed(out, samples[0]);
        }
        Kind::Verbatim => {
            out.put(0b0000_0010, 8);
            for sample in samples {
                signed(out, *sample);
            }
        }
        Kind::Fixed(order) => {
            out.put((0b001000 | order as u64) << 1, 8);
            for sample in &samples[..order] {
                signed(out, *sample);
            }
            residuals(samples, order, residual);
            out.put(0b01, 2);
            out.put(plan.partition_order as u64, 4);
            let mut at = 0;
            for index in 0..1usize << plan.partition_order {
                let length = partition_length(count, order, plan.partition_order, index);
                let values = &residual[at..at + length];
                let (k, _) = rice(values.iter().map(|v| fold(*v)).sum(), length);
                out.put(k as u64, 5);
                for value in values {
                    let folded = fold(*value);
                    out.put_unary(folded >> k);
                    if k > 0 {
                        out.put(folded & ((1u64 << k) - 1), k);
                    }
                }
                at += length;
            }
        }
    }
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    /// Bits already used in the last byte, 0-7.
    used: u32,
}
impl BitWriter {
    fn clear(&mut self) {
        self.bytes.clear();
        self.used = 0;
    }
    fn put(&mut self, value: u64, bits: u32) {
        let mut left = bits;
        while left > 0 {
            if self.used == 0 {
                self.bytes.push(0);
            }
            let room = 8 - self.used;
            let take = room.min(left);
            let chunk = ((value >> (left - take)) & ((1u64 << take) - 1)) as u8;
            *self.bytes.last_mut().expect("a byte was pushed") |= chunk << (room - take);
            self.used = (self.used + take) % 8;
            left -= take;
        }
    }
    fn put_unary(&mut self, mut zeros: u64) {
        while zeros >= 32 {
            self.put(0, 32);
            zeros -= 32;
        }
        self.put(1, zeros as u32 + 1);
    }
    fn align(&mut self) {
        self.used = 0;
    }
    /// The frame number, in the extended UTF-8 scheme FLAC borrows.
    fn put_utf8(&mut self, value: u64) {
        if value < 0x80 {
            return self.put(value, 8);
        }
        let continuation = match value {
            0..=0x7ff => 1,
            0x800..=0xffff => 2,
            0x1_0000..=0x1f_ffff => 3,
            0x20_0000..=0x3ff_ffff => 4,
            0x400_0000..=0x7fff_ffff => 5,
            _ => 6,
        };
        let lead_bits = 6 - continuation;
        let prefix = (0xffu64 << (lead_bits + 1)) & 0xff;
        let lead = if lead_bits == 0 {
            0
        } else {
            (value >> (6 * continuation)) & ((1 << lead_bits) - 1)
        };
        self.put(prefix | lead, 8);
        for index in (0..continuation).rev() {
            self.put(0x80 | ((value >> (6 * index)) & 0x3f), 8);
        }
    }
}
fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// RFC 1321. FLAC signs the decoded audio with it so a player can prove the file intact.
struct Md5 {
    state: [u32; 4],
    block: [u8; 64],
    filled: usize,
    length: u64,
}
impl Md5 {
    fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }
    fn update(&mut self, bytes: &[u8]) {
        self.length = self.length.wrapping_add(bytes.len() as u64);
        for byte in bytes {
            self.block[self.filled] = *byte;
            self.filled += 1;
            if self.filled == 64 {
                self.compress();
                self.filled = 0;
            }
        }
    }
    fn finish(mut self) -> [u8; 16] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_le_bytes());
        let mut out = [0; 16];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
    fn compress(&mut self) {
        const SHIFTS: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        let mut words = [0u32; 16];
        for (word, chunk) in words.iter_mut().zip(self.block.as_chunks::<4>().0) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let [mut a, mut b, mut c, mut d] = self.state;
        for (i, shift) in SHIFTS.iter().enumerate() {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let constant = ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(constant)
                .wrapping_add(words[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(*shift));
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: [u8; 16]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    #[test]
    fn md5_matches_the_rfc_vectors() {
        assert_eq!(hex(Md5::new().finish()), "d41d8cd98f00b204e9800998ecf8427e");
        let mut md5 = Md5::new();
        md5.update(b"abc");
        assert_eq!(hex(md5.finish()), "900150983cd24fb0d6963f7d28e17f72");
        let mut md5 = Md5::new();
        md5.update(
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
        );
        assert_eq!(hex(md5.finish()), "57edf4a22be3c955ac49da2e2107b67a");
    }
    #[test]
    fn bits_checksums_and_frame_numbers_are_written_as_flac_reads_them() {
        let mut bits = BitWriter::default();
        bits.put(0b101, 3);
        bits.put_unary(2);
        bits.put(0x1ff, 9);
        assert_eq!(bits.bytes, vec![0b1010_0111, 0b1111_1110]);
        let utf8 = |value: u64| {
            let mut bits = BitWriter::default();
            bits.put_utf8(value);
            bits.bytes
        };
        assert_eq!(utf8(0x24), vec![0x24]);
        assert_eq!(utf8(0xa2), "\u{a2}".as_bytes());
        assert_eq!(utf8(0x20ac), "\u{20ac}".as_bytes());
        assert_eq!(utf8(0x1_0348), "\u{10348}".as_bytes());
        assert_eq!(crc8(b"123456789"), 0xf4);
        assert_eq!(crc16(b"123456789"), 0xfee8);
    }
    #[test]
    fn silence_loud_noise_and_a_ramp_each_take_the_cheapest_subframe() {
        let mut scratch = vec![];
        assert_eq!(plan(&[7; 64], 16, &mut scratch).kind, Kind::Constant);
        let ramp: Vec<i64> = (0..64).map(|i| i * 3).collect();
        assert_eq!(plan(&ramp, 16, &mut scratch).kind, Kind::Fixed(2));
        let mut seed = 9u32;
        let noise: Vec<i64> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 16) as i64 - 32768
            })
            .collect();
        assert_eq!(plan(&noise, 16, &mut scratch).kind, Kind::Verbatim);
    }
    #[test]
    fn full_scale_24_bit_extremes_survive_a_round_trip() {
        let mut file = std::io::Cursor::new(Vec::new());
        let mut encoder = Encoder::new(&mut file, 48000, 24).unwrap();
        let mut sent = vec![];
        for i in 0..5000i32 {
            let left = if i % 2 == 0 { 8_388_607 } else { -8_388_608 };
            let right = (i * 1291) % 8_388_607 - 4_000_000;
            encoder.write(left).unwrap();
            encoder.write(right).unwrap();
            sent.push([left as f32 / 8_388_608.0, right as f32 / 8_388_608.0]);
        }
        encoder.finish().unwrap();
        let decoded = crate::audio::decode(file.into_inner(), Some("flac")).unwrap();
        assert_eq!(decoded.frames, sent);
    }
}
