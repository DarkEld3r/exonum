#[macro_use(message)]
extern crate exonum;

use exonum::messages::{Message, RawTransaction, Error as MessageError};
use exonum::crypto::{PublicKey, Hash, hash};
use exonum::storage::{Error, View as StorageView};
use exonum::blockchain::{Service, Transaction};
use exonum::node::State;

pub const TIMESTAMPING_SERVICE: u16 = 129;
pub const TIMESTAMPING_TRANSACTION_MESSAGE_ID: u16 = 128;

message! {
    TimestampTx {
        const TYPE = TIMESTAMPING_SERVICE;
        const ID = TIMESTAMPING_TRANSACTION_MESSAGE_ID;
        const SIZE = 40;

        pub_key:        &PublicKey  [00 => 32]
        data:           &[u8]       [32 => 40]
    }
}

pub struct TimestampingService {}

impl TimestampingService {
    pub fn new() -> TimestampingService {
        TimestampingService {}
    }
}

impl Transaction for TimestampTx {
    fn verify(&self) -> bool {
        Message::verify(self, self.pub_key())
    }

    fn execute(&self, _: &StorageView) -> Result<(), Error> {
        Ok(())
    }

    fn raw(&self) -> &RawTransaction {
        Message::raw(self)
    }

    fn clone_box(&self) -> Box<Transaction> {
        Box::new(self.clone())
    }

    fn hash(&self) -> Hash {
        Message::hash(self)
    }
}

impl Service for TimestampingService {
    fn service_id(&self) -> u16 {
        TIMESTAMPING_SERVICE
    }

    fn handle_genesis_block(&self, _: &StorageView) -> Result<(), Error> {
        Ok(())
    }

    fn state_hash(&self, _: &StorageView) -> Result<Hash, Error> {
        Ok(hash(&[]))
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, MessageError> {
        if raw.message_type() != TIMESTAMPING_TRANSACTION_MESSAGE_ID {
            return Err(MessageError::IncorrectMessageType { message_type: raw.message_type() });
        }

        TimestampTx::from_raw(raw).map(|tx| Box::new(tx) as Box<Transaction>)
    }

    fn handle_commit(&self,
                     _: &StorageView,
                     _: &mut State)
                     -> Result<Vec<Box<Transaction>>, Error> {
        Ok(Vec::new())
    }
}