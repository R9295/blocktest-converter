//! Simplified/minimal test input format.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level simplified input.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct SimplifiedInput {
    pub version: String,
    pub fork: String,
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    pub accounts: BTreeMap<String, MinimalAccount>,
    pub blocks: Vec<MinimalBlock>,
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
    pub balance: String,
    pub nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub storage: BTreeMap<String, String>,
    #[serde(rename = "privateKey", skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

/// A transaction in the simplified format.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct MinimalTx {
    pub from: String,
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub value: String,
    pub gas: String,
    #[serde(rename = "gasPrice")]
    pub gas_price: String,
    pub nonce: String,
    pub data: String,
    #[serde(rename = "accessList", default, skip_serializing_if = "Option::is_none")]
    pub access_list: Option<Vec<MinimalAccessListEntry>>,
    /// EIP-2718 transaction type: 0=legacy, 1=access-list, 2=dynamic-fee, 3=blob, 4=set-code.
    pub tx_type: u8,
    /// Max priority fee (tip) for type-2/3/4 txs.
    #[serde(
        rename = "maxPriorityFee",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_priority_fee: Option<String>,
    /// Max fee per gas for type-2/3/4 txs.
    #[serde(rename = "maxFee", default, skip_serializing_if = "Option::is_none")]
    pub max_fee: Option<String>,
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
    pub v: String,
    pub r: String,
    pub s: String,
}

/// Environment settings.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct MinimalEnv {
    #[serde(rename = "currentCoinbase")]
    pub current_coinbase: String,
    #[serde(rename = "currentDifficulty")]
    pub current_difficulty: String,
    #[serde(rename = "currentGasLimit")]
    pub current_gas_limit: String,
    #[serde(rename = "currentNumber")]
    pub current_number: String,
    #[serde(rename = "currentTimestamp")]
    pub current_timestamp: String,
    #[serde(rename = "currentBaseFee")]
    pub current_base_fee: String,
    #[serde(rename = "currentRandom")]
    pub current_random: String,
    #[serde(
        rename = "currentExcessBlobGas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub current_excess_blob_gas: Option<String>,
}
