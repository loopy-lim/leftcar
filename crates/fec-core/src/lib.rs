//! Small, dependency-free Reed-Solomon recovery core for media fragments.
//!
//! The wire layer groups at most eight media shards and adds up to four parity
//! shards. Every shard is fixed-width for coding and begins with a two-byte
//! payload length so the final fragment can be restored without ambiguity.

use std::fmt;

const MAX_DATA_SHARDS: usize = 8;
pub const MAX_PARITY_SHARDS: usize = 4;
const LEGACY_MAX_PARITY_SHARDS: usize = 2;
const LENGTH_PREFIX: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FecError {
    EmptyGroup,
    TooManyDataShards(usize),
    InvalidParityCount(usize),
    ShardTooLarge(usize),
    InvalidShardCount { expected: usize, actual: usize },
    InvalidShardWidth(usize),
    TooManyMissingShards(usize),
    InvalidShardLength,
    SingularMatrix,
}

impl fmt::Display for FecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGroup => write!(f, "FEC group must contain data"),
            Self::TooManyDataShards(count) => {
                write!(
                    f,
                    "FEC group has {count} data shards; maximum is {MAX_DATA_SHARDS}"
                )
            }
            Self::InvalidParityCount(count) => {
                write!(f, "FEC parity count {count} exceeds {MAX_PARITY_SHARDS}")
            }
            Self::ShardTooLarge(size) => write!(f, "FEC shard payload {size} exceeds u16 length"),
            Self::InvalidShardCount { expected, actual } => {
                write!(f, "FEC expected {expected} shards, received {actual}")
            }
            Self::InvalidShardWidth(width) => write!(f, "invalid FEC shard width {width}"),
            Self::TooManyMissingShards(count) => write!(f, "FEC has {count} missing shards"),
            Self::InvalidShardLength => write!(f, "invalid FEC shard length prefix"),
            Self::SingularMatrix => write!(f, "FEC recovery matrix is singular"),
        }
    }
}

impl std::error::Error for FecError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedGroup {
    pub k: usize,
    pub width: usize,
    pub data: Vec<Vec<u8>>,
    pub parity: Vec<Vec<u8>>,
}

impl EncodedGroup {
    pub fn all_shards(&self) -> Vec<Option<Vec<u8>>> {
        self.data
            .iter()
            .chain(self.parity.iter())
            .cloned()
            .map(Some)
            .collect()
    }
}

/// Encode one group. Groups with fewer than eight data shards use one parity
/// shard; a full group uses two. This keeps the short tail bounded without
/// spending a second datagram on a tiny final group.
pub fn encode_group(data: &[Vec<u8>]) -> Result<EncodedGroup, FecError> {
    encode_group_exact(data, parity_count(data.len()))
}

/// Encode with a selected parity ceiling. Short tail groups never emit more
/// than `k - 1` parity shards, while full groups may use all four rows.
pub fn encode_group_with_parity(
    data: &[Vec<u8>],
    parity_count: usize,
) -> Result<EncodedGroup, FecError> {
    if parity_count == 0 || parity_count > MAX_PARITY_SHARDS {
        return Err(FecError::InvalidParityCount(parity_count));
    }
    let effective = parity_count.min(data.len().saturating_sub(1));
    encode_group_exact(data, effective)
}

fn encode_group_exact(data: &[Vec<u8>], parity_count: usize) -> Result<EncodedGroup, FecError> {
    let k = data.len();
    if k == 0 {
        return Err(FecError::EmptyGroup);
    }
    if k > MAX_DATA_SHARDS {
        return Err(FecError::TooManyDataShards(k));
    }
    let max_payload = data
        .iter()
        .map(Vec::len)
        .max()
        .ok_or(FecError::EmptyGroup)?;
    if max_payload > usize::from(u16::MAX) {
        return Err(FecError::ShardTooLarge(max_payload));
    }
    let width = LENGTH_PREFIX + max_payload;
    let data_shards = data
        .iter()
        .map(|payload| pack_shard(payload, width))
        .collect::<Result<Vec<_>, _>>()?;
    let mut parity = vec![vec![0; width]; parity_count];
    for (parity_index, output) in parity.iter_mut().enumerate() {
        let row = generator_row(k + parity_index, k);
        for byte_index in 0..width {
            output[byte_index] = data_shards
                .iter()
                .enumerate()
                .map(|(column, shard)| gf_mul(row[column], shard[byte_index]))
                .fold(0, |value, term| value ^ term);
        }
    }
    Ok(EncodedGroup {
        k,
        width,
        data: data_shards,
        parity,
    })
}

/// Recover all data payloads from a group containing at least `k` shards.
/// `received` is ordered as data shards followed by parity shards.
pub fn decode_group(
    received: Vec<Option<Vec<u8>>>,
    k: usize,
    width: usize,
) -> Result<Vec<Vec<u8>>, FecError> {
    decode_group_with_max_parity(received, k, width, parity_count(k))
}

/// Decode from a receiver allocation that can hold up to `max_parity` rows.
/// Missing tail slots are valid, which lets a four-slot receiver decode an
/// older two-parity sender without changing the media wire format.
pub fn decode_group_with_max_parity(
    received: Vec<Option<Vec<u8>>>,
    k: usize,
    width: usize,
    max_parity: usize,
) -> Result<Vec<Vec<u8>>, FecError> {
    if !(1..=MAX_DATA_SHARDS).contains(&k) {
        return Err(FecError::TooManyDataShards(k));
    }
    if max_parity > MAX_PARITY_SHARDS {
        return Err(FecError::InvalidParityCount(max_parity));
    }
    if width < LENGTH_PREFIX {
        return Err(FecError::InvalidShardWidth(width));
    }
    let expected = k + max_parity.min(k.saturating_sub(1));
    if received.len() != expected {
        return Err(FecError::InvalidShardCount {
            expected,
            actual: received.len(),
        });
    }
    let missing_data = received[..k].iter().filter(|shard| shard.is_none()).count();
    if received.iter().flatten().any(|shard| shard.len() != width) {
        return Err(FecError::InvalidShardWidth(width));
    }

    let selected = received
        .iter()
        .enumerate()
        .filter_map(|(index, shard)| shard.as_ref().map(|_| index))
        .take(k)
        .collect::<Vec<_>>();
    if selected.len() < k {
        return Err(FecError::TooManyMissingShards(missing_data));
    }
    let matrix = selected
        .iter()
        .map(|index| generator_row(*index, k))
        .collect::<Vec<_>>();
    let inverse = invert_matrix(matrix)?;
    let selected_shards = selected
        .iter()
        .map(|index| received[*index].as_ref().expect("selected is present"))
        .collect::<Vec<_>>();
    let mut data_shards = vec![vec![0; width]; k];
    for (row, output) in data_shards.iter_mut().enumerate() {
        for byte_index in 0..width {
            output[byte_index] = (0..k)
                .map(|column| gf_mul(inverse[row][column], selected_shards[column][byte_index]))
                .fold(0, |value, term| value ^ term);
        }
    }
    data_shards
        .iter()
        .map(|shard| unpack_shard(shard))
        .collect()
}

pub fn parity_count(k: usize) -> usize {
    match k {
        0 | 1 => 0,
        2..=7 => 1,
        _ => LEGACY_MAX_PARITY_SHARDS,
    }
}

pub fn pack_shard(payload: &[u8], width: usize) -> Result<Vec<u8>, FecError> {
    if width < LENGTH_PREFIX || payload.len() > width - LENGTH_PREFIX {
        return Err(FecError::InvalidShardWidth(width));
    }
    if payload.len() > usize::from(u16::MAX) {
        return Err(FecError::ShardTooLarge(payload.len()));
    }
    let mut shard = vec![0; width];
    shard[..LENGTH_PREFIX].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    shard[LENGTH_PREFIX..LENGTH_PREFIX + payload.len()].copy_from_slice(payload);
    Ok(shard)
}

pub fn unpack_shard(shard: &[u8]) -> Result<Vec<u8>, FecError> {
    if shard.len() < LENGTH_PREFIX {
        return Err(FecError::InvalidShardLength);
    }
    let length = usize::from(u16::from_be_bytes([shard[0], shard[1]]));
    if length > shard.len() - LENGTH_PREFIX {
        return Err(FecError::InvalidShardLength);
    }
    Ok(shard[LENGTH_PREFIX..LENGTH_PREFIX + length].to_vec())
}

fn generator_row(index: usize, k: usize) -> Vec<u8> {
    if index < k {
        return (0..k).map(|column| u8::from(column == index)).collect();
    }
    // Preserve the legacy parity rows 0 and 1 (bases 1 and 2), then extend
    // them as a Vandermonde series over the same column points. Bases 4 and 8
    // make every combination of up to four missing data columns invertible.
    let base = 1u8 << (index - k);
    (0..k).map(|column| gf_pow(base, column)).collect()
}

fn invert_matrix(matrix: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, FecError> {
    let size = matrix.len();
    let mut augmented = vec![vec![0; size * 2]; size];
    for row in 0..size {
        if matrix[row].len() != size {
            return Err(FecError::SingularMatrix);
        }
        augmented[row][..size].copy_from_slice(&matrix[row]);
        augmented[row][size + row] = 1;
    }
    for column in 0..size {
        let pivot = (column..size)
            .find(|row| augmented[*row][column] != 0)
            .ok_or(FecError::SingularMatrix)?;
        augmented.swap(column, pivot);
        let scale = gf_inv(augmented[column][column]);
        for value in &mut augmented[column] {
            *value = gf_mul(*value, scale);
        }
        for row in 0..size {
            if row == column {
                continue;
            }
            let factor = augmented[row][column];
            if factor == 0 {
                continue;
            }
            let mut offset = 0;
            while offset < size * 2 {
                augmented[row][offset] ^= gf_mul(factor, augmented[column][offset]);
                offset += 1;
            }
        }
    }
    Ok(augmented
        .into_iter()
        .map(|row| row.into_iter().skip(size).collect())
        .collect())
}

fn gf_pow(base: u8, exponent: usize) -> u8 {
    if exponent == 0 {
        return 1;
    }
    let mut result = 1;
    for _ in 0..exponent {
        result = gf_mul(result, base);
    }
    result
}

/// Reference bit-loop multiply over GF(2^8) with reduction polynomial
/// x^8 + x^4 + x^3 + x^2 + 1 (0x1d). This is the exact arithmetic the host
/// shim and every shipped viewer has always used; the tables below only
/// memoize it.
const fn gf_mul_reference(left: u8, right: u8) -> u8 {
    if left == 0 || right == 0 {
        return 0;
    }
    let mut a = left;
    let mut b = right;
    let mut result = 0;
    let mut step = 0;
    while step < 8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let carry = a & 0x80 != 0;
        a <<= 1;
        if carry {
            a ^= 0x1d;
        }
        b >>= 1;
        step += 1;
    }
    result
}

/// 64 KiB multiply table over the same GF(256) field as the macOS host shim
/// (`CaptureSession+UdpPacket.swift fecMultiplyTable`), so both sides keep
/// producing bit-identical parity and recovery shards. Const-evaluated: the
/// viewer RX recovery path pays one indexed load per byte instead of the
/// eight-step bit loop (~0.3–1ms per recovered group on mobile).
static MULTIPLY_TABLE: [u8; 256 * 256] = {
    let mut table = [0u8; 256 * 256];
    let mut left = 0usize;
    while left < 256 {
        let mut right = 0usize;
        while right < 256 {
            table[(left << 8) | right] = gf_mul_reference(left as u8, right as u8);
            right += 1;
        }
        left += 1;
    }
    table
};

/// Multiplicative inverses for every nonzero element (0 stays unmapped; the
/// callers assert nonzero). Replaces the pow-254 chain on the recovery path.
static INVERSE_TABLE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut value = 1usize;
    while value < 256 {
        let mut inverse = 0u8;
        let mut candidate = 1usize;
        while candidate < 256 {
            if MULTIPLY_TABLE[(value << 8) | candidate] == 1 {
                inverse = candidate as u8;
                break;
            }
            candidate += 1;
        }
        table[value] = inverse;
        value += 1;
    }
    table
};

#[inline]
fn gf_mul(left: u8, right: u8) -> u8 {
    MULTIPLY_TABLE[(usize::from(left) << 8) | usize::from(right)]
}

fn gf_inv(value: u8) -> u8 {
    assert_ne!(value, 0, "zero has no multiplicative inverse");
    INVERSE_TABLE[usize::from(value)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_shards_survive_two_losses() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 1200]).collect();
        let encoded = encode_group(&data).expect("encode");
        assert_eq!(encoded.k, 8);
        assert_eq!(encoded.parity.len(), 2);

        let mut received = encoded.all_shards();
        received[0] = None;
        received[9] = None;
        let recovered = decode_group(received, encoded.k, encoded.width).expect("recover");
        assert_eq!(recovered[0], data[0]);
    }

    #[test]
    fn three_losses_are_beyond_capacity() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 300]).collect();
        let encoded = encode_group(&data).expect("encode");
        let mut received = encoded.all_shards();
        received[1] = None;
        received[4] = None;
        received[9] = None;
        assert!(decode_group(received, encoded.k, encoded.width).is_err());
    }

    #[test]
    fn short_tail_group_uses_shrunk_rs() {
        let data: Vec<Vec<u8>> = (0..3).map(|i| vec![i as u8; 100]).collect();
        let encoded = encode_group(&data).expect("encode");
        assert_eq!(encoded.k, 3);
        assert_eq!(encoded.parity.len(), 1);
    }

    #[test]
    fn four_parity_recovers_four_missing_data_shards() {
        let data: Vec<Vec<u8>> = (0..8)
            .map(|i| (0..1200).map(|offset| (i * 17 + offset) as u8).collect())
            .collect();
        let encoded = encode_group_with_parity(&data, 4).expect("encode four parity");
        assert_eq!(encoded.parity.len(), 4);

        let mut received = encoded.all_shards();
        for index in [0, 2, 5, 7] {
            received[index] = None;
        }
        let recovered = decode_group_with_max_parity(received, 8, encoded.width, 4)
            .expect("recover four data shards");
        assert_eq!(recovered, data);
    }

    #[test]
    fn four_slot_receiver_decodes_legacy_two_parity_group() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 240]).collect();
        let encoded = encode_group(&data).expect("legacy encode");
        let mut received = encoded.all_shards();
        received.resize(12, None);
        received[1] = None;
        received[6] = None;

        let recovered = decode_group_with_max_parity(received, 8, encoded.width, 4)
            .expect("new decoder accepts legacy parity slots");
        assert_eq!(recovered, data);
    }

    #[test]
    fn four_parity_rejects_five_missing_data_shards() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 240]).collect();
        let encoded = encode_group_with_parity(&data, 4).expect("encode four parity");
        let mut received = encoded.all_shards();
        for slot in received.iter_mut().take(5) {
            *slot = None;
        }
        assert!(decode_group_with_max_parity(received, 8, encoded.width, 4).is_err());
    }

    /// Independent log/exp construction of GF(256) mod 0x11d, sharing no code
    /// with the table build, so the sweep below is a real cross-check.
    fn gf_mul_log_exp(left: u8, right: u8) -> u8 {
        if left == 0 || right == 0 {
            return 0;
        }
        // 2 is primitive for x^8+x^4+x^3+x^2+1; walk the whole multiplicative
        // group by repeated xtime (multiply by 2 with 0x11d reduction).
        let mut log = [0u8; 256];
        let mut exp = [0u8; 255];
        let mut value: u16 = 1;
        for (power, slot) in exp.iter_mut().enumerate() {
            *slot = value as u8;
            log[value as usize] = power as u8;
            value = (value << 1) ^ if value & 0x80 != 0 { 0x11d } else { 0 };
        }
        exp[(usize::from(log[usize::from(left)]) + usize::from(log[usize::from(right)])) % 255]
    }

    /// Bit-equivalence proof: the shipped multiply table must agree with the
    /// original bit loop AND with the independent log/exp construction for
    /// every one of the 65536 operand pairs.
    #[test]
    fn multiply_table_matches_brute_force_for_every_operand_pair() {
        for left in 0..=255u8 {
            for right in 0..=255u8 {
                let table = gf_mul(left, right);
                assert_eq!(
                    table,
                    gf_mul_reference(left, right),
                    "table != bit loop at {left}x{right}"
                );
                assert_eq!(
                    table,
                    gf_mul_log_exp(left, right),
                    "table != log/exp at {left}x{right}"
                );
            }
        }
    }

    /// The inverse table must be the exact two-sided inverse of the multiply
    /// table and must agree with the legacy pow-254 computation.
    #[test]
    fn inverse_table_is_exact_for_every_nonzero_element() {
        for value in 1..=255u8 {
            let inverse = gf_inv(value);
            assert_eq!(gf_mul(value, inverse), 1, "{value} * inverse != 1");
            assert_eq!(gf_mul(inverse, value), 1, "inverse * {value} != 1");
            assert_eq!(inverse, gf_pow(value, 254), "inverse != pow-254 at {value}");
        }
    }

    /// Identity anchors for the 0x11d field the host shim's bit loop defines.
    #[test]
    fn field_anchors_match_the_host_shim_table() {
        assert_eq!(gf_mul(1, 1), 1);
        assert_eq!(gf_mul(2, 2), 4);
        // xtime under the 0x1d reduction: the spilled bit cancels the 0x100
        // term, so 128 * 2 == 0x1d.
        assert_eq!(gf_mul(2, 128), 29);
        assert_eq!(gf_mul(2, 64), 128);
        assert_eq!(gf_mul(0, 0xab), 0);
        assert_eq!(gf_mul(0xab, 0), 0);
        assert_eq!(gf_inv(1), 1);
    }
}
