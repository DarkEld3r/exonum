use super::super::messages::{RequestMessage, Message, RequestPropose, RequestTransactions,
                             RequestPrevotes, RequestPrecommits, RequestBlock, Block};
use super::super::blockchain::{Blockchain, View};
use super::super::storage::{Map, List};
use super::super::events::Channel;
use super::{NodeHandler, ExternalMessage, NodeTimeout};


const REQUEST_ALIVE: i64 = 3_000_000_000; // 3 seconds

impl<B, S> NodeHandler<B, S>
    where B: Blockchain,
          S: Channel<ApplicationEvent = ExternalMessage<B>, Timeout = NodeTimeout> + Clone
{
    pub fn handle_request(&mut self, msg: RequestMessage) {
        // Request are sended to us
        if msg.to() != &self.public_key {
            return;
        }

        // FIXME: we should use some epsilon for checking lifetime < 0
        let lifetime = match (self.channel.get_time() - msg.time()).num_nanoseconds() {
            Some(nanos) => nanos,
            None => {
                // Incorrect time into message
                return;
            }
        };

        // Incorrect time of the request
        if lifetime < 0 || lifetime > REQUEST_ALIVE {
            return;
        }

        if !msg.verify(msg.from()) {
            return;
        }

        match msg {
            RequestMessage::Propose(msg) => self.handle_request_propose(msg),
            RequestMessage::Transactions(msg) => self.handle_request_txs(msg),
            RequestMessage::Prevotes(msg) => self.handle_request_prevotes(msg),
            RequestMessage::Precommits(msg) => self.handle_request_precommits(msg),
            RequestMessage::Peers(msg) => self.handle_request_peers(msg),
            RequestMessage::Block(msg) => self.handle_request_block(msg),
        }
    }

    pub fn handle_request_propose(&mut self, msg: RequestPropose) {
        if msg.height() != self.state.height() {
            return;
        }

        let propose = if msg.height() == self.state.height() {
            self.state.propose(msg.propose_hash()).map(|p| p.message().raw().clone())
        } else {
            return;
        };

        if let Some(propose) = propose {
            self.send_to_peer(*msg.from(), &propose);
        }
    }

    pub fn handle_request_txs(&mut self, msg: RequestTransactions) {
        debug!("HANDLE TRANSACTIONS REQUEST!!!");
        let view = self.blockchain.view();
        for hash in msg.txs() {
            let tx = self.state
                .transactions()
                .get(hash)
                .cloned()
                .or_else(|| view.transactions().get(hash).unwrap());

            if let Some(tx) = tx {
                self.send_to_peer(*msg.from(), tx.raw());
            }
        }
    }

    pub fn handle_request_prevotes(&mut self, msg: RequestPrevotes) {
        if msg.height() != self.state.height() {
            return;
        }

        let prevotes = if let Some(prevotes) = self.state
            .prevotes(msg.round(), *msg.propose_hash()) {
            prevotes.values().map(|p| p.raw().clone()).collect()
        } else {
            Vec::new()
        };

        for prevote in prevotes {
            self.send_to_peer(*msg.from(), &prevote);
        }
    }

    pub fn handle_request_precommits(&mut self, msg: RequestPrecommits) {
        if msg.height() > self.state.height() {
            return;
        }

        let precommits = if msg.height() == self.state.height() {
            if let Some(precommits) = self.state
                .precommits(msg.round(), *msg.block_hash()) {
                precommits.values().map(|p| p.raw().clone()).collect()
            } else {
                Vec::new()
            }
        } else {
            // msg.height < state.height
            self.blockchain
                .view()
                .precommits(msg.block_hash())
                .values()
                .unwrap()
                .iter()
                .map(|p| p.raw().clone())
                .collect()
        };

        for precommit in precommits {
            self.send_to_peer(*msg.from(), &precommit);
        }
    }

    pub fn handle_request_block(&mut self, msg: RequestBlock) {
        debug!("Handle block request with height:{}, our height: {}",
               msg.height(),
               self.state.height());
        if msg.height() >= self.state.height() {
            return;
        }

        let view = self.blockchain.view();
        let height = msg.height();
        let block_hash = view.heights().get(height).unwrap().unwrap();

        let block = view.blocks().get(&block_hash).unwrap().unwrap();
        let precommits = view.precommits(&block_hash)
            .values()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let transactions = view.block_txs(height)
            .values()
            .unwrap()
            .iter()
            .map(|tx_hash| view.transactions().get(tx_hash).unwrap().unwrap())
            .map(|p| p.raw().clone())
            .collect::<Vec<_>>();

        let block_msg = Block::new(&self.public_key,
                                   msg.from(),
                                   self.channel.get_time(),
                                   block,
                                   precommits,
                                   transactions,
                                   &self.secret_key);
        self.send_to_peer(*msg.from(), block_msg.raw());
    }
}
