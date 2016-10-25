extern crate rand;

use std::net::SocketAddr;

use rand::Rng;

use super::super::blockchain::Blockchain;
use super::super::messages::{Any, RawMessage, Connect, Status, Message, RequestPeers};
use super::{NodeHandler, RequestData};

use super::super::events::Channel;
use super::{ExternalMessage, NodeTimeout};

impl<B, S> NodeHandler<B, S>
    where B: Blockchain,
          S: Channel<ApplicationEvent = ExternalMessage<B>, Timeout = NodeTimeout> + Clone
{
    pub fn handle_message(&mut self, raw: RawMessage) {
        // TODO: check message headers (network id, protocol version)
        // FIXME: call message.verify method
        //     if !raw.verify() {
        //         return;
        //     }
        let msg = Any::from_raw(raw).unwrap();
        match msg {
            Any::Connect(msg) => self.handle_connect(msg),
            Any::Status(msg) => self.handle_status(msg),
            Any::Transaction(message) => self.handle_tx(message),
            Any::Consensus(message) => self.handle_consensus(message),
            Any::Request(message) => self.handle_request(message),
            Any::Block(message) => self.handle_block(message),
        }
    }

    pub fn handle_connected(&mut self, addr: &SocketAddr) {
        debug!("Connected to: {}", addr);
        let message = self.state.our_connect_message().clone();
        self.send_to_addr(addr, message.raw());
    }

    pub fn handle_disconnected(&mut self, addr: &SocketAddr) {
        debug!("Disconnected from: {}", addr);
        let need_reconnect = self.state.remove_peer_with_addr(addr);
        if need_reconnect {
            self.connect(addr);
        }
    }

    pub fn handle_connect(&mut self, message: Connect) {
        // TODO add spam protection
        let address = message.addr();
        if address == self.state.our_connect_message().addr() {
            return;
        }
        info!("Received connect message from {}", address);

        // Check if we have another connect message from peer with the given public_key
        let public_key = *message.pub_key();
        let mut need_connect = true;
        if let Some(saved_message) = self.state.peers().get(&public_key) {
            if saved_message.time() > message.time() {
                warn!("Received weird connection message from {}", address);
                return;
            }
            need_connect = !(saved_message.addr() == message.addr() &&
                             saved_message.time() == message.time());
        }
        self.state.add_peer(public_key, message);

        if need_connect {
            // TODO: reduce double sending of connect message
            info!("Establish connection with {}", address);
            self.connect(&address);
        }
    }

    pub fn handle_status(&mut self, msg: Status) {
        // Handle message from future height
        if msg.height() > self.state.height() {
            // Check validator height info
            // FIXME: make sure that validator id < validator count
            if self.state.validator_height(msg.validator()) >= msg.height() {
                return;
            }
            // Verify validator if and signature
            let peer = match self.state.public_key_of(msg.validator()) {
                // Incorrect signature of message
                Some(public_key) => {
                    if !msg.verify(public_key) {
                        return;
                    }
                    *public_key
                }
                // Incorrect validator id
                None => return,
            };
            // Update validator height
            self.state.set_validator_height(msg.validator(), msg.height());
            // Request commit
            self.request(RequestData::Commit, peer);
        }

        // TODO: remove this?
        // // Handle message from current height
        // if msg.height() == self.state.height() {
        //     // Request propose or txs
        //     self.request_propose_or_txs(ctx, msg.propose_hash(), msg.validator());

        //     // Request precommits
        //     if !self.state.has_majority_precommits(msg.round(),
        //                                           *msg.propose_hash(),
        //                                           *msg.block_hash()) {
        //         let data = RequestData::Precommits(msg.round(),
        //                                           *msg.propose_hash(),
        //                                           *msg.block_hash());
        //         self.request(ctx, data, msg.validator());
        //     }
        // }
    }

    pub fn handle_request_peers(&mut self, msg: RequestPeers) {
        let peers: Vec<Connect> = self.state.peers().iter().map(|(_, b)| b.clone()).collect();
        for peer in peers {
            self.send_to_peer(*msg.from(), peer.raw());
        }
    }

    pub fn handle_status_timeout(&mut self) {
        if let Some(hash) = self.blockchain.last_hash().unwrap() {
            // Send status
            let status = Status::new(self.state.id(),
                                     self.state.height(),
                                     &hash,
                                     &self.secret_key);
            self.broadcast(status.raw());
            debug!("Send status: {:?}", status);
        }
        self.add_status_timeout();
    }

    pub fn handle_peer_exchange_timeout(&mut self) {
        if !self.state.peers().is_empty() {
            let to = self.state.peers().len();
            let gen_peer_id = || -> usize {
                let mut rng = rand::thread_rng();
                rng.gen_range(0, to)
            };

            let peer = self.state
                .peers()
                .iter()
                .map(|x| x.1.clone())
                .skip(gen_peer_id())
                .next()
                .unwrap();
            let peer = peer.clone();
            let msg = RequestPeers::new(&self.public_key,
                                        peer.pub_key(),
                                        self.channel.get_time(),
                                        &self.secret_key);
            self.send_to_peer(*peer.pub_key(), msg.raw());

            debug!("request peers from peer with addr {:?}", peer.addr());
        }
        self.add_peer_exchange_timeout();
    }
}
