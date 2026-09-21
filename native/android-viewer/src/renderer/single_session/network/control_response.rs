use crate::media_crypto::MediaSessionCrypto;

pub(crate) fn consume_control_datagram(
    crypto: &MediaSessionCrypto,
    packet: &mut [u8],
    consume_plaintext: impl FnOnce(&[u8]) -> bool,
) -> bool {
    // Use the media receiver's same crypto/window: control replies can arrive
    // on either socket, and neither path may parse unauthenticated bytes.
    let Some(plaintext) = crypto.open_into(packet) else {
        return false;
    };
    consume_plaintext(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_protocol::{parse_ack, parse_latency_probe_response, InputAck};
    use crate::media_crypto::test_media_key;
    use crate::renderer::single_session::health::ControlHealthState;
    use secure_channel::DatagramSealer;
    use std::net::UdpSocket;
    use std::time::Duration;

    fn crypto_pair() -> (MediaSessionCrypto, DatagramSealer) {
        let key = test_media_key(12);
        (
            MediaSessionCrypto::new(key),
            DatagramSealer::new(secure_channel::media_keys(&key).s2c),
        )
    }

    // Match CaptureSession.sendLatencyProbeResponse's big-endian LCP2 body.
    fn host_latency_response(sequence: u32) -> Vec<u8> {
        let mut response = b"LCP2".to_vec();
        response.extend_from_slice(&sequence.to_be_bytes());
        for time in [1_000_u64, 1_005, 1_006] {
            response.extend_from_slice(&time.to_be_bytes());
        }
        response
    }

    fn host_ack(sequence: u32) -> Vec<u8> {
        let mut ack = b"LCA1".to_vec();
        ack.extend_from_slice(&sequence.to_be_bytes());
        ack.push(0); // Keep remote input disabled; only parse the Host response.
        ack
    }

    fn socket() -> UdpSocket {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        socket
    }

    fn receive(host: &UdpSocket, receiver: &UdpSocket, sealed: &[u8]) -> Vec<u8> {
        host.send_to(sealed, receiver.local_addr().unwrap())
            .unwrap();
        let mut packet = vec![0_u8; 256];
        let (size, peer) = receiver.recv_from(&mut packet).unwrap();
        assert_eq!(peer, host.local_addr().unwrap());
        packet.truncate(size);
        packet
    }

    #[test]
    fn sealed_host_latency_reply_acknowledges_the_pending_probe() {
        let (crypto, host_tx) = crypto_pair();
        let (host, control_socket) = (socket(), socket());
        let mut health = ControlHealthState::default();
        health.probe_sent(7);
        let sealed = host_tx.seal(&host_latency_response(7)).unwrap();
        let mut packet = receive(&host, &control_socket, &sealed);
        let mut response_seen = None;
        assert!(consume_control_datagram(
            &crypto,
            &mut packet,
            |plaintext| {
                let Some(response) = parse_latency_probe_response(plaintext) else {
                    return false;
                };
                response_seen = Some(response);
                health.probe_acknowledged(response.sequence)
            }
        ));
        let response = response_seen.unwrap();
        assert_eq!(response.sequence, 7);
        assert_eq!(response.viewer_send_ms, 1_000);
        assert_eq!(response.host_receive_ms, 1_005);
        assert_eq!(response.host_send_ms, 1_006);
        assert!(
            !health.probe_acknowledged(7),
            "the pending probe was consumed"
        );
    }

    #[test]
    fn media_and_control_sockets_share_replay_protection() {
        let (crypto, host_tx) = crypto_pair();
        let (host, media_socket, control_socket) = (socket(), socket(), socket());
        let ack = host_tx.seal(&host_ack(42)).unwrap();
        let mut control_packet = receive(&host, &control_socket, &ack);
        assert!(consume_control_datagram(
            &crypto,
            &mut control_packet,
            |plaintext| {
                parse_ack(plaintext)
                    == Some(InputAck {
                        sequence: 42,
                        enabled: Some(false),
                    })
            }
        ));
        let mut media_replay = receive(&host, &media_socket, &ack);
        assert!(crypto.open_into(&mut media_replay).is_none());

        let next_ack = host_tx.seal(&host_ack(43)).unwrap();
        let mut media_packet = receive(&host, &media_socket, &next_ack);
        assert_eq!(
            parse_ack(crypto.open_into(&mut media_packet).unwrap()),
            Some(InputAck {
                sequence: 43,
                enabled: Some(false)
            })
        );
        let mut control_replay = receive(&host, &control_socket, &next_ack);
        assert!(!consume_control_datagram(
            &crypto,
            &mut control_replay,
            |_| { panic!("a duplicate must not reach the response parser") }
        ));
    }

    #[test]
    fn forged_response_is_not_dispatched_or_consumed() {
        let (crypto, host_tx) = crypto_pair();
        let sealed = host_tx.seal(&host_ack(44)).unwrap();
        let mut forged = sealed.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert!(!consume_control_datagram(&crypto, &mut forged, |_| {
            panic!("a forged response must not reach the parser")
        }));
        let mut authentic = sealed.clone();
        assert!(consume_control_datagram(
            &crypto,
            &mut authentic,
            |plaintext| { parse_ack(plaintext).is_some_and(|ack| ack.sequence == 44) }
        ));
        let mut duplicate = sealed;
        assert!(!consume_control_datagram(&crypto, &mut duplicate, |_| {
            panic!("a duplicate must not reach the parser")
        }));
    }

    #[test]
    fn plaintext_and_truncated_responses_never_reach_the_parser() {
        let (crypto, host_tx) = crypto_pair();
        let mut plaintext = host_latency_response(7);
        assert!(!consume_control_datagram(&crypto, &mut plaintext, |_| {
            panic!("plaintext must not bypass authentication")
        }));
        let sealed = host_tx.seal(&host_latency_response(7)).unwrap();
        let mut truncated = sealed[..12].to_vec();
        assert!(!consume_control_datagram(&crypto, &mut truncated, |_| {
            panic!("truncated response must not reach the parser")
        }));
    }
}
