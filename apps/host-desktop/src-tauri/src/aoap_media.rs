//! Identity-scoped subscriptions to one accessory's media dispatcher.
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct Subscriber {
    identity: Arc<()>,
    sender: SyncSender<Vec<u8>>,
}

#[derive(Clone, Default)]
pub(crate) struct MediaSubscribers(Arc<Mutex<Option<Subscriber>>>);

pub(crate) struct UsbMediaChannel {
    pub sender: SyncSender<usb_mux::MuxFrame>,
    pub receiver: Receiver<Vec<u8>>,
    pub lease: MediaLease,
}

pub(crate) struct MediaLease {
    subscribers: MediaSubscribers,
    identity: Arc<()>,
}

impl Drop for MediaLease {
    fn drop(&mut self) {
        self.subscribers.clear(&self.identity);
    }
}

impl MediaSubscribers {
    pub(crate) fn acquire(
        &self,
        outgoing: SyncSender<usb_mux::MuxFrame>,
    ) -> Option<UsbMediaChannel> {
        let mut slot = self.0.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        let (sender, receiver) = mpsc::sync_channel(256);
        let identity = Arc::new(());
        *slot = Some(Subscriber {
            identity: identity.clone(),
            sender,
        });
        Some(UsbMediaChannel {
            sender: outgoing,
            receiver,
            lease: MediaLease {
                subscribers: self.clone(),
                identity,
            },
        })
    }

    fn clear(&self, identity: &Arc<()>) {
        let mut slot = self.0.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.identity, identity))
        {
            *slot = None;
        }
    }

    pub(crate) fn dispatch(&self, payload: Vec<u8>) {
        let Some(subscriber) = self.0.lock().unwrap().clone() else {
            return;
        };
        self.deliver(subscriber, payload, || {});
    }

    fn deliver(&self, subscriber: Subscriber, mut payload: Vec<u8>, mut on_full: impl FnMut()) {
        loop {
            {
                let slot = self.0.lock().unwrap();
                if !slot
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.identity, &subscriber.identity))
                {
                    return;
                }
                // The identity check and nonblocking send linearize against lease
                // revocation; never hold this lock through a queue wait.
                match subscriber.sender.try_send(payload) {
                    Ok(()) => return,
                    Err(TrySendError::Full(pending)) => payload = pending,
                    Err(TrySendError::Disconnected(_)) => {
                        drop(slot);
                        self.clear(&subscriber.identity);
                        return;
                    }
                }
            }
            // A no-op in production; tests observe Full outside the slot lock
            // to coordinate revocation while the old receiver remains alive.
            on_full();
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing() -> SyncSender<usb_mux::MuxFrame> {
        mpsc::sync_channel(1).0
    }

    #[test]
    fn old_lease_and_dispatch_cleanup_preserve_successor() {
        let slot = MediaSubscribers::default();
        let old = slot.acquire(outgoing()).unwrap();
        let snapshot = slot.0.lock().unwrap().clone().unwrap();
        slot.clear(&snapshot.identity);
        let new = slot.acquire(outgoing()).unwrap();
        // Models a dispatcher send failing after the old receiver was revoked.
        slot.clear(&snapshot.identity);
        drop(old);
        slot.dispatch(b"successor".to_vec());
        assert_eq!(
            new.receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"successor"
        );
        assert!(slot.acquire(outgoing()).is_none());
    }

    #[test]
    fn accessory_replacement_does_not_redirect_old_lease_drop() {
        let accessory_a = MediaSubscribers::default();
        let accessory_b = MediaSubscribers::default();
        let old = accessory_a.acquire(outgoing()).unwrap();
        let new = accessory_b.acquire(outgoing()).unwrap();
        drop(old);
        accessory_b.dispatch(b"B".to_vec());
        assert_eq!(new.receiver.recv().unwrap(), b"B");
        assert!(accessory_a.acquire(outgoing()).is_some());
    }

    #[test]
    fn revoked_full_queue_does_not_block_dispatch_or_send_old_payload_to_successor() {
        let slot = MediaSubscribers::default();
        let old = slot.acquire(outgoing()).unwrap();
        let snapshot = slot.0.lock().unwrap().clone().unwrap();
        let mut queued = 0;
        while snapshot.sender.try_send(vec![1]).is_ok() {
            queued += 1;
        }
        assert!(queued > 0);
        let dispatcher_slot = slot.clone();
        let (ready, full_observed) = mpsc::channel();
        let (resume, revoked) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut first_full = true;
            dispatcher_slot.deliver(snapshot, b"old".to_vec(), || {
                if first_full {
                    first_full = false;
                    ready.send(()).unwrap();
                    revoked.recv().unwrap();
                }
            });
            done.send(()).unwrap();
        });
        full_observed.recv_timeout(Duration::from_secs(1)).unwrap();
        // Keep the old receiver alive and undrained throughout revocation and
        // dispatcher completion: disconnection cannot release a blocking send.
        drop(old.lease);
        let new = slot.acquire(outgoing()).unwrap();
        resume.send(()).unwrap();
        let result = completed.recv_timeout(Duration::from_secs(1));
        if result.is_err() {
            // Let a broken blocking-send implementation terminate before RED.
            drop(old.receiver);
            worker.join().unwrap();
            panic!("lease revocation did not release a Full delivery with a live receiver");
        }
        worker.join().unwrap();
        assert!(new.receiver.try_recv().is_err());
        for _ in 0..queued {
            assert_eq!(old.receiver.try_recv().unwrap(), vec![1]);
        }
        assert!(
            old.receiver.try_recv().is_err(),
            "old pending payload entered the revoked queue"
        );
        slot.dispatch(b"new".to_vec());
        assert_eq!(new.receiver.recv().unwrap(), b"new");
    }

    #[test]
    fn dropping_receiver_clears_only_its_lease_and_allows_reacquisition() {
        let slot = MediaSubscribers::default();
        let channel = slot.acquire(outgoing()).unwrap();
        drop(channel.receiver);
        slot.dispatch(vec![1]);
        let new = slot.acquire(outgoing()).unwrap();
        drop(channel.lease);
        slot.dispatch(vec![2]);
        assert_eq!(
            new.receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            vec![2]
        );
    }
}
