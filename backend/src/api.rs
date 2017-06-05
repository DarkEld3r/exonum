//! Cryptocurrency API.

use serde::Serialize;
use serde_json::to_value;
use router::Router;
use iron::prelude::*;
use bodyparser;
use params::{Params, Value};

use std::fmt;

use exonum::api::{Api, ApiError};
use exonum::node::TransactionSend;
use exonum::messages::BlockProof;
use exonum::crypto::{HexValue, PublicKey, Hash};
use exonum::storage::{StorageValue, List, Map, Proofnode, RootProofNode};
use exonum::blockchain::{self, Blockchain};

use super::tx_metarecord::TxMetaRecord;
use super::wallet::Wallet;
use super::{CRYPTOCURRENCY_SERVICE_ID, CurrencySchema, CurrencyTx};

/// TODO: Add documentation.
#[derive(Debug, Serialize)]
pub struct MPTProofTemplate<V: Serialize> {
    mpt_proof: RootProofNode<Hash>,
    value: V,
}

/// TODO: Add documentation.
#[derive(Debug, Serialize)]
pub struct MTProofTemplate<V: Serialize> {
    mt_proof: Proofnode<TxMetaRecord>,
    values: Vec<V>,
}

/// Wallet information.
#[derive(Debug, Serialize)]
pub struct WalletInfo {
    block_info: BlockProof,
    wallet: MPTProofTemplate<RootProofNode<Wallet>>,
    wallet_history: Option<MTProofTemplate<CurrencyTx>>,
}

/// TODO: Add documentation.
#[derive(Clone)]
pub struct CryptocurrencyApi<T: TransactionSend + Clone> {
    /// Exonum blockchain.
    pub blockchain: Blockchain,
    /// Channel for transactions.
    pub channel: T,
}

impl<T> CryptocurrencyApi<T>
    where T: TransactionSend + Clone
{
    fn wallet_info(&self, pub_key: &PublicKey) -> Result<WalletInfo, ApiError> {
        let view = self.blockchain.view();
        let general_schema = blockchain::Schema::new(&view);
        let currency_schema = CurrencySchema::new(&view);

        let max_height = general_schema.block_hashes_by_height().len()? - 1;
        let block_proof = general_schema.block_and_precommits(max_height)?.unwrap();
        let state_hash = *block_proof.block.state_hash(); //debug code

        let wallet_path: MPTProofTemplate<RootProofNode<Wallet>>;
        let wallet_history: Option<MTProofTemplate<CurrencyTx>>;

        let to_wallets_table: RootProofNode<Hash> =
            general_schema
                .get_proof_to_service_table(CRYPTOCURRENCY_SERVICE_ID, 0)?;

        {
            let wallets_root_hash = currency_schema.wallets().root_hash()?; //debug code
            let check_result = to_wallets_table.verify_root_proof_consistency(
                Blockchain::service_table_unique_key(CRYPTOCURRENCY_SERVICE_ID, 0),
                state_hash); //debug code
            debug_assert_eq!(wallets_root_hash, *check_result.unwrap().unwrap());
        }

        let to_specific_wallet: RootProofNode<Wallet> =
            currency_schema.wallets().construct_path_to_key(pub_key)?;
        wallet_path = MPTProofTemplate {
            mpt_proof: to_wallets_table,
            value: to_specific_wallet,
        };

        wallet_history = match currency_schema.wallet(pub_key)? {
            Some(wallet) => {
                let history = currency_schema.wallet_history(pub_key);
                let history_len = history.len()?;
                debug_assert!(history_len >= 1);
                debug_assert_eq!(history_len, wallet.history_len());
                let tx_records: Vec<TxMetaRecord> = history.values()?;
                let transactions_table = general_schema.transactions();
                let mut txs: Vec<CurrencyTx> = Vec::with_capacity(tx_records.len());
                for record in tx_records {
                    let raw_message = transactions_table.get(record.tx_hash())?.unwrap();
                    txs.push(CurrencyTx::from(raw_message));
                }
                let to_transaction_hashes: Proofnode<TxMetaRecord> =
                    history.construct_path_for_range(0, history_len)?;
                let path_to_transactions = MTProofTemplate {
                    mt_proof: to_transaction_hashes,
                    values: txs,
                };
                Some(path_to_transactions)
            }
            None => None,
        };
        let res = WalletInfo {
            block_info: block_proof,
            wallet: wallet_path,
            wallet_history: wallet_history,
        };
        Ok(res)
    }

    fn transaction(&self, tx: CurrencyTx) -> Result<Hash, ApiError> {
        let tx_hash = tx.hash();
        match self.channel.send(tx) {
            Ok(_) => Ok(tx_hash),
            Err(e) => Err(ApiError::Events(e)),
        }
    }
}

impl<T: Clone + TransactionSend> fmt::Debug for CryptocurrencyApi<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CryptocurrencyApi {{}}")
    }
}

impl<T> Api for CryptocurrencyApi<T>
    where T: 'static + TransactionSend + Clone
{
    fn wire(&self, router: &mut Router) {
        let self_ = self.clone();
        let wallet_info = move |req: &mut Request| -> IronResult<Response> {
            let map = req.get_ref::<Params>().unwrap();
            match map.find(&["pubkey"]) {
                Some(&Value::String(ref pub_key_string)) => {
                    let public_key = PublicKey::from_hex(pub_key_string)
                        .map_err(ApiError::FromHex)?;
                    let info = self_.wallet_info(&public_key)?;
                    self_.ok_response(&to_value(&info).unwrap())
                }
                _ => Err(ApiError::IncorrectRequest)?,
            }
        };

        let self_ = self.clone();
        let transaction = move |req: &mut Request| -> IronResult<Response> {
            match req.get::<bodyparser::Struct<CurrencyTx>>() {
                Ok(Some(transaction)) => {
                    let tx_hash = self_.transaction(transaction)?;
                    let json = TxResponse { tx_hash: tx_hash };
                    self_.ok_response(&to_value(&json).unwrap())
                }
                Ok(None) => Err(ApiError::IncorrectRequest)?,
                Err(e) => {
                    error!("Incorrect CurrencyTx request body received {}", e);
                    Err(ApiError::IncorrectRequest)?
                }
            }
        };
        let route_post = "/v1/wallets/transaction";
        let route_get = "/v1/wallets/info";
        router.post(&route_post, transaction, "transaction");
        info!("Created post route: {}", route_post);
        router.get(&route_get, wallet_info, "wallet_info");
        info!("Created get route: {}", route_get);
    }
}

#[derive(Serialize, Deserialize)]
struct TxResponse {
    tx_hash: Hash,
}
