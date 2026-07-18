//! Per-client outbound backpressure and latest-observation delivery.

use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::adapter::protocol::{AckMessage, ErrorMessage, ObservationMessage, WelcomeMessage};

pub const CLIENT_RELIABLE_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
pub(super) enum ClientOutbound {
    Ack(AckMessage),
    Error(ErrorMessage),
    Welcome(WelcomeMessage),
    ObservationArc(Arc<ObservationMessage>),
}

#[derive(Clone)]
pub(super) struct ClientOutboundSender {
    reliable_tx: mpsc::Sender<ClientOutbound>,
    observation_tx: watch::Sender<Option<ClientOutbound>>,
    shutdown_tx: watch::Sender<bool>,
}

impl ClientOutboundSender {
    pub(super) fn try_send_reliable(&self, message: ClientOutbound) -> bool {
        match self.reliable_tx.try_send(message) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.request_shutdown();
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub(super) fn publish_observation(&self, observation: ClientOutbound) -> bool {
        if !self.is_live() || self.observation_tx.receiver_count() == 0 {
            self.request_shutdown();
            return false;
        }
        self.observation_tx.send_replace(Some(observation));
        true
    }

    fn request_shutdown(&self) {
        self.shutdown_tx.send_replace(true);
    }

    pub(super) fn is_live(&self) -> bool {
        !*self.shutdown_tx.borrow() && !self.reliable_tx.is_closed()
    }
}

pub(super) fn client_outbound_channel(
    reliable_capacity: usize,
) -> (
    ClientOutboundSender,
    mpsc::Receiver<ClientOutbound>,
    watch::Receiver<Option<ClientOutbound>>,
    watch::Receiver<bool>,
) {
    let (reliable_tx, reliable_rx) = mpsc::channel(reliable_capacity.max(1));
    let (observation_tx, observation_rx) = watch::channel(None);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    (
        ClientOutboundSender {
            reliable_tx,
            observation_tx,
            shutdown_tx,
        },
        reliable_rx,
        observation_rx,
        shutdown_rx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::observation::build_observation;
    use crate::adapter::protocol::create_ack;
    use crate::core::GameSnapshot;

    #[test]
    fn reliable_queue_overflow_requests_disconnect() {
        let (outbound, mut reliable_rx, _observation_rx, mut shutdown_rx) =
            client_outbound_channel(1);

        assert!(outbound.try_send_reliable(ClientOutbound::Ack(create_ack(1, 1))));
        assert!(!outbound.try_send_reliable(ClientOutbound::Ack(create_ack(2, 2))));
        assert!(*shutdown_rx.borrow_and_update());

        let ClientOutbound::Ack(ack) = reliable_rx.try_recv().unwrap() else {
            panic!("expected the first reliable message to remain queued");
        };
        assert_eq!(ack.seq, 1);
    }

    #[test]
    fn observation_slot_keeps_only_the_latest_snapshot() {
        let (outbound, _reliable_rx, mut observation_rx, _shutdown_rx) = client_outbound_channel(1);
        let first = Arc::new(build_observation(10, 0, &GameSnapshot::default(), &[]));
        let latest = Arc::new(build_observation(11, 0, &GameSnapshot::default(), &[]));

        assert!(outbound.publish_observation(ClientOutbound::ObservationArc(first)));
        assert!(outbound.publish_observation(ClientOutbound::ObservationArc(latest)));

        let ClientOutbound::ObservationArc(observation) =
            observation_rx.borrow_and_update().clone().unwrap()
        else {
            panic!("expected a coalesced observation");
        };
        assert_eq!(observation.seq, 11);
    }
}
