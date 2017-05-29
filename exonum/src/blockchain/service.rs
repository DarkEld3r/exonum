use serde_json::Value;
use iron::Handler;
use mount::Mount;

use crypto::{Hash, PublicKey, SecretKey};
use storage::{View, Error as StorageError};
use messages::{Message, RawTransaction, Error as MessageError};
use node::{Node, State, NodeChannel, TxSender};
use node::state::ValidatorState;
use events::Milliseconds;
use blockchain::{StoredConfiguration, ConsensusConfig, Blockchain};

pub trait Transaction: Message + 'static {
    /// Checks the formal correctness of the transaction.
    /// That can be usefull for signature verification.
    /// *This method should not use external data, that is, it must be a pure function!*
    fn verify(&self) -> bool;
    /// Defines transaction executing rules.
    fn execute(&self, view: &View) -> Result<(), StorageError>;
    /// Returns transaction representation in json.
    fn info(&self) -> Value {
        Value::Null
    }
}

/// The main extension point for the `Exonum` framework. Like smart contracts in some other 
/// blockchain platforms, `Exonum` services encapsulate business logic of the blockchain application.
#[allow(unused_variables, unused_mut)]
pub trait Service: Send + Sync + 'static {
    /// Unique service identification for database schema and service messages.
    fn service_id(&self) -> u16;
    /// Unique human readable service name.
    fn service_name(&self) -> &'static str;

    fn state_hash(&self, view: &View) -> Result<Vec<Hash>, StorageError> {
        Ok(Vec::new())
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, MessageError>;

    /// Handles genesis block creation event. 
    /// By this method you can initialize information schema of service 
    /// and generates initial service configuration.
    fn handle_genesis_block(&self, view: &View) -> Result<Value, StorageError> {
        Ok(Value::Null)
    }

    /// Handles commit event. This handler is invoked for each service after commit of the block.
    /// For example service can create some transaction if the specific condition occurred. 
    /// Try not to perform long operations here.
    fn handle_commit(&self, context: &mut NodeState) -> Result<(), StorageError> {
        Ok(())
    }
    /// Returns api handler for public users.
    fn public_api_handler(&self, context: &ApiContext) -> Option<Box<Handler>> {
        None
    }
    /// Returns api handler for maintainers. 
    fn private_api_handler(&self, context: &ApiContext) -> Option<Box<Handler>> {
        None
    }
}

/// The current node state on which the blockchain is running, 
/// or in other words execution context.
#[derive(Debug)]
pub struct NodeState<'a, 'b> {
    state: &'a mut State,
    view: &'b View,
    txs: Vec<Box<Transaction>>,
}

impl<'a, 'b> NodeState<'a, 'b> {
    #[doc(hidden)]
    pub fn new(state: &'a mut State, view: &'b View) -> NodeState<'a, 'b> {
        NodeState {
            state: state,
            view: view,
            txs: Vec::new(),
        }
    }

    /// If the current node is validator returns its state. 
    /// For other nodes return `None`.
    pub fn validator_state(&self) -> &Option<ValidatorState> {
        self.state.validator_state()
    }

    /// Returns the current database snapshot.
    /// You can write your changes to storage, but be very careful. 
    /// Use the write only for caching and never change tables that affect to `state_hash`!
    pub fn view(&self) -> &View {
        self.view
    }

    /// Returns the current blockchain height. This height is 'height of last commited block` + 1.
    pub fn height(&self) -> u64 {
        self.state.height()
    }

    /// Returns the current node round.
    pub fn round(&self) -> u32 {
        self.state.round()
    }

    /// Returns the current list of validators.
    pub fn validators(&self) -> &[PublicKey] {
        self.state.validators()
    }

    /// Returns current node's public key.
    pub fn public_key(&self) -> &PublicKey {
        self.state.public_key()
    }

    /// Returns current node's secret key.
    pub fn secret_key(&self) -> &SecretKey {
        self.state.secret_key()
    }

    /// Returns the actual blockchain global configuration.
    pub fn actual_config(&self) -> &StoredConfiguration {
        self.state.config()
    }

    /// Returns the config of consensus.
    pub fn consensus_config(&self) -> &ConsensusConfig {
        self.state.consensus_config()
    }

    /// Returns service specific global variables as json value.
    pub fn service_config(&self, service: &Service) -> &Value {
        let id = service.service_id();
        self.state
            .services_config()
            .get(&format!("{}", id))
            .unwrap()
    }

    /// Adds transaction to the queue.
    /// After the services handle commit event these transactions will be broadcasted by node.
    pub fn add_transaction<T: Transaction>(&mut self, tx: T) {
        assert!(tx.verify());
        self.txs.push(Box::new(tx));
    }

    #[doc(hidden)]
    // FIXME remove it!
    pub fn transactions(self) -> Vec<Box<Transaction>> {
        self.txs
    }

    #[doc(hidden)]
    // FIXME remove it!
    pub fn update_config(&mut self, new_config: StoredConfiguration) {
        self.state.update_config(new_config)
    }

    #[doc(hidden)]
    // FIXME remove it!
    pub fn propose_timeout(&self) -> Milliseconds {
        self.state.propose_timeout()
    }

    #[doc(hidden)]
    // FIXME remove it!
    pub fn set_propose_timeout(&mut self, timeout: Milliseconds) {
        self.state.set_propose_timeout(timeout)
    }
}

#[derive(Debug)]
pub struct ApiContext {
    blockchain: Blockchain,
    node_channel: TxSender<NodeChannel>,
    public_key: PublicKey,
    secret_key: SecretKey,
}

impl ApiContext {
    pub fn new(node: &Node) -> ApiContext {
        let handler = node.handler();
        ApiContext {
            blockchain: handler.blockchain.clone(),
            node_channel: node.channel(),
            public_key: *node.state().public_key(),
            secret_key: node.state().secret_key().clone(),
        }
    }

    pub fn blockchain(&self) -> &Blockchain {
        &self.blockchain
    }

    pub fn node_channel(&self) -> &TxSender<NodeChannel> {
        &self.node_channel
    }
    
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn secret_key(&self) -> &SecretKey {
        &self.secret_key
    }

    pub fn mount_public_api(&self) -> Mount {
        self.blockchain.mount_public_api(self)
    }

    pub fn mount_private_api(&self) -> Mount {
        self.blockchain.mount_private_api(self)
    }
}