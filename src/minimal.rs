//! Simplified/minimal test input format.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level simplified input describing a complete block test.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct SimplifiedInput {
    /// Format version (currently `"1"`).
    pub version: String,
    /// Ethereum fork name (e.g. `"Osaka"`, `"Cancun"`, `"Prague"`).
    pub fork: String,
    /// Network chain ID.
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    /// Pre-state accounts keyed by hex address.
    pub accounts: BTreeMap<String, MinimalAccount>,
    /// Ordered list of blocks to execute.
    pub blocks: Vec<MinimalBlock>,
    /// Block-level environment settings.
    pub env: MinimalEnv,
}

/// A block containing transactions and optional withdrawals.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalBlock {
    pub transactions: Vec<MinimalTx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawals: Option<Vec<MinimalWithdrawal>>,
    #[serde(rename = "expectException", skip_serializing_if = "Option::is_none")]
    pub expect_exception: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coinbase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(
        rename = "parentBeaconBlockRoot",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_beacon_block_root: Option<String>,
    #[serde(
        rename = "baseFeePerGas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub base_fee_per_gas: Option<String>,
    #[serde(
        rename = "excessBlobGas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub excess_blob_gas: Option<String>,
}

/// An EIP-4895 withdrawal.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalWithdrawal {
    pub index: String,
    #[serde(rename = "validatorIndex")]
    pub validator_index: String,
    pub address: String,
    pub amount: String,
}

/// An account in the pre-state.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalAccount {
    /// Account balance in wei (hex).
    pub balance: String,
    /// Account nonce (hex).
    pub nonce: String,
    /// Contract bytecode (hex). Omit or set to `"0x"` for EOAs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Storage slot → value mapping (both hex, 32-byte keys).
    pub storage: BTreeMap<String, String>,
    /// ECDSA private key (hex, 32 bytes). Required for transaction senders
    /// and EIP-7702 authorization signers.
    #[serde(rename = "privateKey", skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

/// A transaction in the simplified format.
///
/// The converter signs transactions automatically using the sender's private
/// key from the accounts map.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalTx {
    /// Sender address (hex). Must exist in the accounts map with a `privateKey`.
    pub from: String,
    /// Transaction chain ID (hex).
    #[serde(rename = "chainId")]
    pub chain_id: String,
    /// Recipient address (hex). `None` for contract creation.
    /// Required for type 3 (blob) and type 4 (set-code) transactions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Transfer value in wei (hex).
    pub value: String,
    /// Gas limit (hex).
    pub gas: String,
    /// Gas price (hex). Required for type 0 (legacy) and type 1 (access-list).
    #[serde(rename = "gasPrice", default, skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
    /// Transaction nonce (hex).
    pub nonce: String,
    /// Calldata (hex).
    pub data: String,
    /// EIP-2930 access list. Optional for all tx types.
    #[serde(rename = "accessList", default, skip_serializing_if = "Option::is_none")]
    pub access_list: Option<Vec<MinimalAccessListEntry>>,
    /// EIP-2718 transaction type: 0=legacy, 1=access-list, 2=dynamic-fee, 3=blob, 4=set-code.
    #[serde(rename = "txType")]
    pub tx_type: u8,
    /// Max priority fee (tip) for type-2/3/4 txs.
    #[serde(rename = "maxPriorityFee")]
    pub max_priority_fee: String,
    /// Max fee per gas for type-2/3/4 txs.
    #[serde(rename = "maxFee")]
    pub max_fee: String,
    /// Max fee per blob gas for type-3 (blob) txs.
    #[serde(
        rename = "maxFeePerBlobGas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_fee_per_blob_gas: Option<String>,
    /// Blob versioned hashes for type-3 (blob) txs.
    #[serde(
        rename = "blobVersionedHashes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub blob_versioned_hashes: Option<Vec<String>>,
    /// Authorization list for type-4 (set-code) txs.
    #[serde(
        rename = "authorizationList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub authorization_list: Option<Vec<MinimalAuthorization>>,
}

/// An EIP-2930/EIP-1559 access-list item.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalAccessListEntry {
    pub address: String,
    #[serde(rename = "storageKeys")]
    pub storage_keys: Vec<String>,
}

/// An EIP-7702 authorization tuple.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalAuthorization {
    #[serde(rename = "chainId")]
    pub chain_id: String,
    pub address: String,
    pub nonce: String,
    /// Address of the signer account (must have a `privateKey` in the accounts map).
    pub signer: String,
}

/// Block-level environment settings applied to genesis and/or execution blocks.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct MinimalEnv {
    /// Block coinbase / fee recipient address (hex, 20 bytes).
    #[serde(rename = "currentCoinbase")]
    pub current_coinbase: String,
    /// Block difficulty (hex). Set to `"0x0"` for post-merge forks.
    #[serde(rename = "currentDifficulty")]
    pub current_difficulty: String,
    /// Block gas limit (hex).
    #[serde(rename = "currentGasLimit")]
    pub current_gas_limit: String,
    /// Starting block number (hex). Genesis is always 0; blocks start at 1.
    #[serde(rename = "currentNumber")]
    pub current_number: String,
    /// Base timestamp for the first block (hex).
    #[serde(rename = "currentTimestamp")]
    pub current_timestamp: String,
    /// Base fee per gas applied to the genesis header (hex).
    #[serde(rename = "currentBaseFee")]
    pub current_base_fee: String,
    /// RANDAO / prevRandao mix hash (hex, 32 bytes).
    #[serde(rename = "currentRandom")]
    pub current_random: String,
    /// Excess blob gas for the genesis header (hex). Optional; defaults to 0.
    #[serde(
        rename = "currentExcessBlobGas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub current_excess_blob_gas: Option<String>,
}
