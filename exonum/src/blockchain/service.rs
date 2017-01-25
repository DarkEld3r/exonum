use serde_json::Value;

use ::crypto::Hash;
use ::storage::{View, Error as StorageError};
use ::messages::{Message, RawTransaction, Error as MessageError};
use ::node::State;

pub trait Transaction: Message + 'static {
    fn verify(&self) -> bool;
    fn execute(&self, view: &View) -> Result<(), StorageError>;
    fn info(&self) -> Value {
        Value::Null
    }
}

pub trait Service: Send + Sync + 'static {
    fn service_id(&self) -> u16;

    fn state_hash(&self, _: &View) -> Option<Result<Hash, StorageError>> {
        None
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, MessageError>;

    fn handle_genesis_block(&self, _: &View) -> Result<(), StorageError> {
        Ok(())
    }

    fn handle_commit(&self,
                     _: &View,
                     _: &mut State)
                     -> Result<Vec<Box<Transaction>>, StorageError> {
        Ok(Vec::new())
    }
}