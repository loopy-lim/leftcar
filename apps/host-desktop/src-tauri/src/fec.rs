//! Host-side FEC grouping for already-fragmented media access units.

use crate::wire;

const DATA_SHARDS_PER_GROUP: usize = 8;

/// Build parity datagrams for an AU's media payload fragments. The group
/// metadata keeps the AU and fragment base explicit, so an AU larger than
/// eight fragments can be recovered in independent bounded groups.
pub fn parity_datagrams_for_payloads(
    au_id: u16,
    host_wall_ms: u64,
    fragments: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    let fragment_count = fragments.len();
    let mut output = Vec::new();
    for (base, group) in fragments.chunks(DATA_SHARDS_PER_GROUP).enumerate() {
        let Ok(encoded) = fec_core::encode_group(group) else {
            return Vec::new();
        };
        output.extend(wire::parity_datagrams_for_group(
            au_id,
            encoded.k as u8,
            (base * DATA_SHARDS_PER_GROUP) as u16,
            fragment_count as u16,
            host_wall_ms,
            &encoded.parity,
        ));
    }
    output
}

/// Convert the existing `G` datagrams into the payload shape used by the FEC
/// core, then emit `P` datagrams. Malformed media datagrams fail closed.
pub fn parity_datagrams_for_media(
    au_id: u16,
    host_wall_ms: u64,
    datagrams: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    let payloads = datagrams
        .iter()
        .map(|datagram| {
            let header = if datagram.get(7..9) == Some(b"L2") {
                wire::FRAME_HEADER_V2_LEN
            } else if datagram.get(7..9) == Some(b"LT") {
                wire::FRAME_HEADER_V1_LEN
            } else {
                return None;
            };
            datagram.get(header..).map(<[u8]>::to_vec)
        })
        .collect::<Option<Vec<_>>>();
    let Some(payloads) = payloads else {
        return Vec::new();
    };
    parity_datagrams_for_payloads(au_id, host_wall_ms, &payloads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_eight_fragments_and_preserves_au_identity() {
        let fragments = (0..9)
            .map(|index| vec![index as u8; 500])
            .collect::<Vec<_>>();
        let parity = parity_datagrams_for_payloads(77, 1_000, &fragments);
        assert_eq!(parity.len(), 2);
        assert!(parity
            .iter()
            .all(|datagram| datagram.len() <= wire::MAX_DATAGRAM));
        assert_eq!(parity[0][0], b'P');
        assert_eq!(u16::from_le_bytes([parity[0][1], parity[0][2]]), 77);
        assert_eq!(parity[0][3], 8);
        assert_eq!(u16::from_be_bytes([parity[0][5], parity[0][6]]), 0);
        assert_eq!(u16::from_be_bytes([parity[0][7], parity[0][8]]), 9);
    }
}
