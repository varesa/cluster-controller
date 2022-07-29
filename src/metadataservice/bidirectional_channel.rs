use futures::channel::mpsc::{channel, Receiver, Sender};

const BUF_SIZE: usize = 128;

pub struct ChannelEndpoint<T> {
    pub tx: Sender<T>,
    pub rx: Receiver<T>,
}

pub fn bidirectional_channel<T>() -> (ChannelEndpoint<T>, ChannelEndpoint<T>) {
    let (channel1_tx, channel1_rx) = channel(BUF_SIZE);
    let (channel2_tx, channel2_rx) = channel(BUF_SIZE);

    let endpoint1 = ChannelEndpoint {
        tx: channel1_tx,
        rx: channel2_rx,
    };

    let endpoint2 = ChannelEndpoint {
        tx: channel2_tx,
        rx: channel1_rx,
    };

    (endpoint1, endpoint2)
}
