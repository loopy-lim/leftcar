//! Small, dependency-free Reed-Solomon recovery core for media fragments.
//!
//! The wire layer groups at most eight media shards and adds up to two parity
//! shards. Every shard is fixed-width for coding and begins with a two-byte
//! payload length so the final fragment can be restored without ambiguity.

use std::fmt;

const MAX_DATA_SHARDS: usize = 8;
const MAX_PARITY_SHARDS: usize = 2;
const LENGTH_PREFIX: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FecError {
    EmptyGroup,
    TooManyDataShards(usize),
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
    let parity_count = parity_count(k);
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
    if !(1..=MAX_DATA_SHARDS).contains(&k) {
        return Err(FecError::TooManyDataShards(k));
    }
    if width < LENGTH_PREFIX {
        return Err(FecError::InvalidShardWidth(width));
    }
    let expected = k + parity_count(k);
    if received.len() != expected {
        return Err(FecError::InvalidShardCount {
            expected,
            actual: received.len(),
        });
    }
    let missing = received.iter().filter(|shard| shard.is_none()).count();
    if missing > parity_count(k) {
        return Err(FecError::TooManyMissingShards(missing));
    }
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
        return Err(FecError::TooManyMissingShards(missing));
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
        _ => MAX_PARITY_SHARDS,
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
    let base = (index - k + 1) as u8;
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

fn gf_mul(left: u8, right: u8) -> u8 {
    if left == 0 || right == 0 {
        return 0;
    }
    let mut a = left;
    let mut b = right;
    let mut result = 0;
    for _ in 0..8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let carry = a & 0x80 != 0;
        a <<= 1;
        if carry {
            a ^= 0x1d;
        }
        b >>= 1;
    }
    result
}

fn gf_inv(value: u8) -> u8 {
    assert_ne!(value, 0, "zero has no multiplicative inverse");
    gf_pow(value, 254)
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
}
