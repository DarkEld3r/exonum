#[macro_use]
extern crate exonum;
extern crate blockchain_explorer;
#[macro_use]
extern crate log;

extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate iron;
extern crate router;
extern crate bodyparser;
extern crate params;

pub mod config_api;
use std::fmt;

use serde::{Serialize, Serializer};

use exonum::messages::Field;
use exonum::blockchain::{Service, Transaction, Schema, NodeState};
use exonum::node::State;
use exonum::crypto::{Signature, PublicKey, hash, Hash, HASH_SIZE};
use exonum::messages::{RawMessage, Message, FromRaw, RawTransaction, Error as MessageError};
use exonum::storage::{StorageValue, List, Map, View, MapTable, MerkleTable, MerklePatriciaTable,
                      Result as StorageResult};
use exonum::blockchain::StoredConfiguration;

pub const CONFIG_SERVICE: u16 = 1;
pub const CONFIG_PROPOSE_MESSAGE_ID: u16 = 0;
pub const CONFIG_VOTE_MESSAGE_ID: u16 = 1;
storage_value! {
    ConfigVotingData {
        const SIZE = 48;

        tx_propose:            TxConfigPropose   [00 => 08]
        votes_history_hash:    &Hash             [08 => 40]
        num_votes:             u64               [40 => 48]
    }
}

impl ConfigVotingData {
    pub fn set_history_hash(&mut self, hash: &Hash) {
        Field::write(&hash, &mut self.raw, 8, 40);
    }
}

impl Serialize for ConfigVotingData {
    fn serialize<S>(&self, ser: &mut S) -> Result<(), S::Error>
        where S: Serializer
    {
        let mut state;
        state = ser.serialize_struct("config voting data", 4)?;
        ser.serialize_struct_elt(&mut state, "tx_propose", self.tx_propose())?;
        ser.serialize_struct_elt(&mut state, "votes_history_hash", self.votes_history_hash())?;
        ser.serialize_struct_elt(&mut state, "num_votes", self.num_votes())?;
        ser.serialize_struct_end(state)
    }
}

message! {
    TxConfigPropose {
        const TYPE = CONFIG_SERVICE;
        const ID = CONFIG_PROPOSE_MESSAGE_ID;
        const SIZE = 40;

        from:           &PublicKey  [00 => 32]
        cfg:            &[u8]       [32 => 40] // serialized config bytes
    }
}

message! {
    TxConfigVote {
        const TYPE = CONFIG_SERVICE;
        const ID = CONFIG_VOTE_MESSAGE_ID;
        const SIZE = 64;

        from:           &PublicKey  [00 => 32]
        cfg_hash:       &Hash       [32 => 64] // hash of config we're voting for
    }
}

#[derive(Clone, PartialEq)]
pub enum ConfigTx {
    ConfigPropose(TxConfigPropose),
    ConfigVote(TxConfigVote),
}

impl Serialize for TxConfigPropose {
    fn serialize<S>(&self, ser: &mut S) -> Result<(), S::Error>
        where S: Serializer
    {
        let mut state;
        state = ser.serialize_struct("config_propose", 4)?;
        ser.serialize_struct_elt(&mut state, "from", self.from())?;
        if let Ok(cfg) = StoredConfiguration::deserialize_err(self.cfg()) {
            ser.serialize_struct_elt(&mut state, "config", cfg)?;
        } else {
            ser.serialize_struct_elt(&mut state, "config", self.cfg())?;
        }
        ser.serialize_struct_elt(&mut state, "signature", self.raw().signature())?;
        ser.serialize_struct_end(state)
    }
}

impl Serialize for TxConfigVote {
    fn serialize<S>(&self, ser: &mut S) -> Result<(), S::Error>
        where S: Serializer
    {
        let mut state;
        state = ser.serialize_struct("vote", 5)?;
        ser.serialize_struct_elt(&mut state, "from", self.from())?;
        ser.serialize_struct_elt(&mut state, "config_hash", self.cfg_hash())?;
        ser.serialize_struct_elt(&mut state, "signature", self.raw().signature())?;
        ser.serialize_struct_end(state)
    }
}

impl Serialize for ConfigTx {
    fn serialize<S>(&self, ser: &mut S) -> Result<(), S::Error>
        where S: Serializer
    {
        match *self {
            ConfigTx::ConfigPropose(ref propose) => propose.serialize(ser),
            ConfigTx::ConfigVote(ref vote) => vote.serialize(ser),
        }
    }
}

#[derive(Default)]
pub struct ConfigurationService {}

pub struct ConfigurationSchema<'a> {
    view: &'a View,
}

impl ConfigTx {
    pub fn from(&self) -> &PublicKey {
        match *self {
            ConfigTx::ConfigPropose(ref msg) => msg.from(),
            ConfigTx::ConfigVote(ref msg) => msg.from(),
        }
    }
}

impl fmt::Debug for ConfigTx {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            ConfigTx::ConfigPropose(ref msg) => write!(fmt, "{:?}", msg),
            ConfigTx::ConfigVote(ref msg) => write!(fmt, "{:?}", msg),
        }
    }
}

impl FromRaw for ConfigTx {
    fn from_raw(raw: RawMessage) -> Result<ConfigTx, MessageError> {
        match raw.message_type() {
            CONFIG_PROPOSE_MESSAGE_ID => {
                Ok(ConfigTx::ConfigPropose(TxConfigPropose::from_raw(raw)?))
            }
            CONFIG_VOTE_MESSAGE_ID => Ok(ConfigTx::ConfigVote(TxConfigVote::from_raw(raw)?)),
            _ => Err(MessageError::IncorrectMessageType { message_type: raw.message_type() }),
        }
    }
}

impl Message for ConfigTx {
    fn raw(&self) -> &RawMessage {
        match *self {
            ConfigTx::ConfigPropose(ref msg) => msg.raw(),
            ConfigTx::ConfigVote(ref msg) => msg.raw(),
        }
    }

    fn verify_signature(&self, public_key: &PublicKey) -> bool {
        match *self {
            ConfigTx::ConfigPropose(ref msg) => msg.verify_signature(public_key),
            ConfigTx::ConfigVote(ref msg) => msg.verify_signature(public_key),
        }
    }

    fn hash(&self) -> Hash {
        match *self {
            ConfigTx::ConfigPropose(ref msg) => Message::hash(msg),
            ConfigTx::ConfigVote(ref msg) => Message::hash(msg),
        }
    }
}

impl<'a> ConfigurationSchema<'a> {
    pub fn new(view: &'a View) -> ConfigurationSchema {
        ConfigurationSchema { view: view }
    }

    /// mapping hash(config) -> TxConfigPropose
    fn config_data
        (&self)
         -> MerklePatriciaTable<MapTable<View, [u8], Vec<u8>>, Hash, ConfigVotingData> {
        MerklePatriciaTable::new(MapTable::new(vec![04], self.view))
    }
    /// mapping validator_id -> TxConfigVote
    fn config_votes(&self,
                    config_hash: &Hash)
                    -> MerkleTable<MapTable<View, [u8], Vec<u8>>, u64, TxConfigVote> {
        let mut prefix = vec![05; 1 + HASH_SIZE];
        prefix[1..].copy_from_slice(config_hash.as_ref());
        MerkleTable::new(MapTable::new(prefix, self.view))
    }

    pub fn put_propose(&self,
                       cfg_hash: &Hash,
                       tx_propose: TxConfigPropose,
                       num_validators: u64)
                       -> StorageResult<()> {
        let votes_table = self.config_votes(cfg_hash);
        debug_assert!(votes_table.is_empty().unwrap());
        let zerovote =
            TxConfigVote::new_with_signature(&PublicKey::zero(), &Hash::zero(), &Signature::zero());
        for _ in 0..(num_validators as usize) {
            votes_table.append(zerovote.clone())?;
        }
        let config_data =
            ConfigVotingData::new(tx_propose, &votes_table.root_hash()?, num_validators);
        let config_data_table = self.config_data();
        debug_assert!(config_data_table.get(cfg_hash).unwrap().is_none());
        config_data_table.put(cfg_hash, config_data)
    }

    pub fn get_propose(&self, cfg_hash: &Hash) -> StorageResult<Option<TxConfigPropose>> {
        let option_config_data = self.config_data().get(cfg_hash)?;
        Ok(option_config_data.map(|config_data| config_data.tx_propose()))
    }

    pub fn put_vote(&self, tx_vote: TxConfigVote) -> StorageResult<()> {
        let cfg_hash = tx_vote.cfg_hash();
        let config_data_table = self.config_data();
        let mut propose_config_data = config_data_table.get(cfg_hash)?
            .expect(&format!("Corresponding propose unexpectedly not found for TxConfigVote:{:?}",
                             &tx_vote));

        let tx_propose = propose_config_data.tx_propose();
        let prev_cfg_hash = StoredConfiguration::deserialize(tx_propose.cfg().to_vec())
            .previous_cfg_hash;
        let general_schema = Schema::new(self.view);
        let prev_cfg = general_schema.configs()
            .get(&prev_cfg_hash)?
            .expect(&format!("Previous cfg:{:?} unexpectedly not found for TxConfigVote:{:?}",
                             prev_cfg_hash,
                             &tx_vote));
        let from: &PublicKey = tx_vote.from();
        let validator_id = prev_cfg.validators
            .iter()
            .position(|pk| pk == from)
            .expect(&format!("See !prev_cfg.validators.contains(self.from()) for \
                              TxConfigVote:{:?}",
                             &tx_vote));

        let votes_for_cfg_table = self.config_votes(cfg_hash);
        votes_for_cfg_table.set(validator_id as u64, tx_vote.clone())?;
        propose_config_data.set_history_hash(&votes_for_cfg_table.root_hash()?);
        config_data_table.put(cfg_hash, propose_config_data)
    }

    pub fn get_votes(&self, cfg_hash: &Hash) -> StorageResult<Vec<Option<TxConfigVote>>> {
        let zerovote =
            TxConfigVote::new_with_signature(&PublicKey::zero(), &Hash::zero(), &Signature::zero());
        let votes_table = self.config_votes(cfg_hash); 
        let votes_values = votes_table.values()?;
        let votes_options = votes_values.into_iter()
            .map(|vote|{
                if vote == zerovote {
                    None
                } else {
                    Some(vote)
                }
            }).collect::<Vec<_>>();
        Ok(votes_options)
    }

    pub fn state_hash(&self) -> StorageResult<Vec<Hash>> {
        Ok(vec![self.config_data().root_hash()?])
    }
}

impl TxConfigPropose {
    fn execute(&self, view: &View) -> StorageResult<()> {
        let blockchain_schema = Schema::new(view);
        let config_schema = ConfigurationSchema::new(view);

        let following_config: Option<StoredConfiguration> =
            blockchain_schema.get_following_configuration()?;
        if let Some(foll_cfg) = following_config {
            error!("Discarding TxConfigPropose: {} as there is an already scheduled next config: \
                    {:?} ",
                   serde_json::to_string(self)?,
                   foll_cfg);
            return Ok(());
        }

        let actual_config: StoredConfiguration = blockchain_schema.get_actual_configuration()?;

        if !actual_config.validators.contains(self.from()) {
            error!("Discarding TxConfigPropose:{} from unknown validator. ",
                   serde_json::to_string(self)?);
            return Ok(());
        }

        let config_candidate = StoredConfiguration::deserialize_err(self.cfg());
        if config_candidate.is_err() {
            error!("Discarding TxConfigPropose:{} which contains config, which cannot be parsed: \
                    {:?}",
                   serde_json::to_string(self)?,
                   config_candidate);
            return Ok(());
        }

        let actual_config_hash = actual_config.hash();
        let config_candidate_body = config_candidate.unwrap();
        if config_candidate_body.previous_cfg_hash != actual_config_hash {
            error!("Discarding TxConfigPropose:{} which does not reference actual config: {:?}",
                   serde_json::to_string(self)?,
                   actual_config);
            return Ok(());
        }

        let current_height = blockchain_schema.current_height()?;
        let actual_from = config_candidate_body.actual_from;
        if actual_from <= current_height {
            error!("Discarding TxConfigPropose:{} which has actual_from height less than or \
                    equal to current: {:?}",
                   serde_json::to_string(self)?,
                   current_height);
            return Ok(());
        }

        let config_hash = config_candidate_body.hash();

        if let Some(tx_propose) = config_schema.get_propose(&config_hash)? {
            error!("Discarding TxConfigPropose:{} which contains an already posted config. \
                    Previous TxConfigPropose:{}",
                   serde_json::to_string(self)?,
                   serde_json::to_string(&tx_propose)?);
            return Ok(());
        }

        config_schema.put_propose(&config_hash,
                         self.clone(),
                         actual_config.validators.len() as u64)?;

        debug!("Put TxConfigPropose:{} to config_proposes table",
               serde_json::to_string(self)?);
        Ok(())
    }
}

impl TxConfigVote {
    fn execute(&self, view: &View) -> StorageResult<()> {
        let blockchain_schema = Schema::new(view);
        let config_schema = ConfigurationSchema::new(view);

        let propose_option = config_schema.get_propose(self.cfg_hash())?;
        if propose_option.is_none() {
            error!("Discarding TxConfigVote:{:?} which references unknown config hash",
                   self);
            return Ok(());
        }

        let following_config: Option<StoredConfiguration> =
            blockchain_schema.get_following_configuration()?;
        if let Some(foll_cfg) = following_config {
            error!("Discarding TxConfigVote: {:?} as there is an already scheduled next config: \
                    {:?} ",
                   self,
                   foll_cfg);
            return Ok(());
        }

        let actual_config: StoredConfiguration = blockchain_schema.get_actual_configuration()?;

        if !actual_config.validators.contains(self.from()) {
            error!("Discarding TxConfigVote:{:?} from unknown validator. ",
                   self);
            return Ok(());
        }

        let referenced_tx_propose = propose_option.unwrap();
        let parsed_config = StoredConfiguration::deserialize_err(referenced_tx_propose.cfg())
            .unwrap();
        let actual_config_hash = actual_config.hash();
        if parsed_config.previous_cfg_hash != actual_config_hash {
            error!("Discarding TxConfigVote:{:?}, whose corresponding TxConfigPropose:{} does \
                    not reference actual config: {:?}",
                   self,
                   serde_json::to_string(&referenced_tx_propose)?,
                   actual_config);
            return Ok(());
        }

        let current_height = blockchain_schema.current_height()?;
        let actual_from = parsed_config.actual_from;
        if actual_from <= current_height {
            error!("Discarding TxConfigVote:{:?}, whose corresponding TxConfigPropose:{} has \
                    actual_from height less than or equal to current: {:?}",
                   self,
                   serde_json::to_string(&referenced_tx_propose)?,
                   current_height);
            return Ok(());
        }

        config_schema.put_vote(self.clone())?;
        debug!("Put TxConfigVote:{:?} to corresponding cfg config_votes table",
               self);

        let mut votes_count = 0;

        for vote_option in config_schema.get_votes(self.cfg_hash())?{
            if let Some(_) = vote_option {
                votes_count += 1;
            }
        }

        if votes_count >= State::byzantine_majority_count(actual_config.validators.len()) {
            blockchain_schema.commit_actual_configuration(parsed_config)?;
        }
        Ok(())
    }
}

impl Transaction for ConfigTx {
    fn verify(&self) -> bool {
        self.verify_signature(self.from())
    }

    fn execute(&self, view: &View) -> StorageResult<()> {
        match *self {
            ConfigTx::ConfigPropose(ref tx) => tx.execute(view),
            ConfigTx::ConfigVote(ref tx) => tx.execute(view),
        }
    }
}

impl ConfigurationService {
    pub fn new() -> ConfigurationService {
        ConfigurationService {}
    }
}

impl Service for ConfigurationService {
    fn service_id(&self) -> u16 {
        CONFIG_SERVICE
    }

    fn state_hash(&self, view: &View) -> StorageResult<Vec<Hash>> {
        let schema = ConfigurationSchema::new(view);
        schema.state_hash()
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, MessageError> {
        ConfigTx::from_raw(raw).map(|tx| Box::new(tx) as Box<Transaction>)
    }

    fn handle_commit(&self, state: &mut NodeState) -> StorageResult<()> {
        let old_cfg = state.actual_config().clone();

        let new_cfg = {
            let schema = Schema::new(state.view());
            schema.get_actual_configuration()?
        };

        if new_cfg != old_cfg {
            info!("Updated node config={:#?}", new_cfg);
            state.update_config(new_cfg);
        }
        Ok(())
    }
}
