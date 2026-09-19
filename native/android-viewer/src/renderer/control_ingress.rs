use crate::media_crypto::MediaSessionCrypto;

/// The dedicated UDP control socket receives the same sealed wire format as
/// the media socket. Keep this boundary host-testable with the real scheduler.
pub(crate) fn consume_control_datagram(
    crypto: &MediaSessionCrypto,
    packet: &mut [u8],
    consume_plaintext: impl FnOnce(&[u8]) -> bool,
) -> bool {
    let Some(plaintext) = crypto.open_into(packet) else {
        return false;
    };
    consume_plaintext(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_protocol::{
        parse_ack, parse_latency_probe_response, InputEvent, InputScheduler,
    };
    use secure_channel::DatagramSealer;

    #[test]
    fn sealed_ack_releases_next_real_input_and_probe_is_readable() {
        let key = [7; 32];
        let viewer = MediaSessionCrypto::new(key);
        let host = DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        let mut scheduler = InputScheduler::new(60);
        scheduler.push(InputEvent::Text { text: "a".into() });
        scheduler.push(InputEvent::Text { text: "b".into() });
        let first = scheduler.next_ready(10_000).unwrap();
        let mut ack = b"LCA1".to_vec();
        ack.extend_from_slice(&first.sequence.to_be_bytes());
        ack.push(1);
        let mut wire = host.seal(&ack).unwrap();
        assert!(
            consume_control_datagram(&viewer, &mut wire, |plain| {
                parse_ack(plain).is_some_and(|ack| scheduler.acknowledge(ack.sequence))
            }),
            "the authenticated ACK must reach the pending input scheduler"
        );
        assert_eq!(
            scheduler.next_ready(10_001).unwrap().sequence,
            first.sequence + 1
        );

        let mut probe = b"LCP2".to_vec();
        probe.extend_from_slice(&1u32.to_be_bytes());
        for timestamp in [100u64, 110, 111] {
            probe.extend_from_slice(&timestamp.to_be_bytes());
        }
        let mut wire = host.seal(&probe).unwrap();
        assert!(consume_control_datagram(&viewer, &mut wire, |plain| {
            parse_latency_probe_response(plain).is_some()
        }));
    }

    #[test]
    fn plaintext_forgery_and_replayed_control_never_reach_dispatch() {
        let key = [8; 32];
        let viewer = MediaSessionCrypto::new(key);
        let host = DatagramSealer::new(secure_channel::media_keys(&key).s2c);
        let sealed = host.seal(b"LCS1\x01").unwrap();
        assert!(consume_control_datagram(
            &viewer,
            &mut sealed.clone(),
            |_| true
        ));
        for mut rejected in [sealed, b"LCS1\x01".to_vec(), vec![0; 40]] {
            assert!(!consume_control_datagram(
                &viewer,
                &mut rejected,
                |_| panic!("untrusted packet reached parser")
            ));
        }
    }
}
