use crate::media_crypto::MediaSessionCrypto;
use crate::net_guard::peer_allowed;
use std::net::SocketAddr;

pub(crate) struct AuthenticatedMedia<'a> {
    pub(crate) plaintext: &'a [u8],
    pub(crate) peer_changed: bool,
}

pub(crate) fn admit_media_datagram<'a>(
    crypto: &MediaSessionCrypto,
    packet: &'a mut [u8],
    peer: SocketAddr,
    expected_host: &str,
    host_peer: &mut Option<SocketAddr>,
    media_since_probe: Option<&mut bool>,
) -> Option<AuthenticatedMedia<'a>> {
    if !peer_allowed(Some(peer), expected_host) {
        return None;
    }
    // Only a fresh packet authenticated by the session can prove liveness or
    // authorize learning a new source port. Replays must leave both untouched.
    let plaintext = crypto.open_into(packet)?;
    if let Some(media_since_probe) = media_since_probe {
        *media_since_probe = true;
    }
    let peer_changed = *host_peer != Some(peer);
    *host_peer = Some(peer);
    Some(AuthenticatedMedia {
        plaintext,
        peer_changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use secure_channel::DatagramSealer;

    fn fixture() -> (MediaSessionCrypto, DatagramSealer) {
        let key = [83; 32];
        (
            MediaSessionCrypto::new(key),
            DatagramSealer::new(secure_channel::media_keys(&key).s2c),
        )
    }

    #[test]
    fn forged_same_host_packets_neither_move_peer_nor_keep_dead_connection_alive() {
        let (crypto, sender) = fixture();
        let original: SocketAddr = "127.0.0.1:11000".parse().unwrap();
        let forged_peer = "127.0.0.1:11001".parse().unwrap();
        let mut host_peer = Some(original);
        for _ in 1..=3 {
            let mut packet = sender.seal(b"G media").unwrap();
            *packet.last_mut().unwrap() ^= 1;
            let mut media_seen = false;
            assert!(admit_media_datagram(
                &crypto,
                &mut packet,
                forged_peer,
                "127.0.0.1",
                &mut host_peer,
                Some(&mut media_seen)
            )
            .is_none());
            assert_eq!(host_peer, Some(original));
            assert!(!media_seen);
        }
    }

    #[test]
    fn replay_cannot_switch_source_port_or_refresh_health() {
        let (crypto, sender) = fixture();
        let original = "127.0.0.1:11000".parse().unwrap();
        let mut host_peer = None;
        let sealed = sender.seal(b"G media").unwrap();
        let mut packet = sealed.clone();
        let mut media_seen = false;
        let admitted = admit_media_datagram(
            &crypto,
            &mut packet,
            original,
            "127.0.0.1",
            &mut host_peer,
            Some(&mut media_seen),
        )
        .unwrap();
        assert_eq!(admitted.plaintext, b"G media");
        assert!(admitted.peer_changed);
        assert!(media_seen);
        let mut replay = sealed;
        media_seen = false;
        assert!(admit_media_datagram(
            &crypto,
            &mut replay,
            "127.0.0.1:11001".parse().unwrap(),
            "127.0.0.1",
            &mut host_peer,
            Some(&mut media_seen)
        )
        .is_none());
        assert_eq!(host_peer, Some(original));
        assert!(!media_seen);
        let mut fresh = sender.seal(b"G replacement").unwrap();
        let successor = "127.0.0.1:11001".parse().unwrap();
        let admitted = admit_media_datagram(
            &crypto,
            &mut fresh,
            successor,
            "127.0.0.1",
            &mut host_peer,
            Some(&mut media_seen),
        )
        .unwrap();
        assert_eq!(admitted.plaintext, b"G replacement");
        assert!(admitted.peer_changed);
        assert_eq!(host_peer, Some(successor));
        assert!(media_seen);
    }

    #[test]
    fn valid_new_peer_is_admitted_and_unexpected_host_is_rejected_before_opening() {
        let (crypto, sender) = fixture();
        let peer = "127.0.0.1:11000".parse().unwrap();
        let sealed = sender.seal(b"LCH1 fixture").unwrap();
        let mut packet = sealed.clone();
        let mut host_peer = None;
        let mut media_seen = false;
        assert!(admit_media_datagram(
            &crypto,
            &mut packet,
            peer,
            "192.0.2.1",
            &mut host_peer,
            Some(&mut media_seen)
        )
        .is_none());
        assert_eq!(host_peer, None);
        assert!(!media_seen);
        let mut packet = sealed;
        let admitted = admit_media_datagram(
            &crypto,
            &mut packet,
            peer,
            "127.0.0.1",
            &mut host_peer,
            Some(&mut media_seen),
        )
        .unwrap();
        assert_eq!(admitted.plaintext, b"LCH1 fixture");
        assert!(admitted.peer_changed);
        assert_eq!(host_peer, Some(peer));
        assert!(media_seen);
    }
}
