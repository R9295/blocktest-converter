//! Deserialization types for Ethereum block test JSON format.
//!
//! This matches the JSON structure produced by go-ethereum's `evm blocktest` and
//! our converter. The top-level is a map from test name to `BlockTest`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level: map from test name -> `BlockTest`
pub type BlockTestFile = BTreeMap<String, BlockTest>;

/// A single block test case.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockTest {
    /// Ordered list of blocks to apply on top of genesis.
    pub blocks: Vec<BtBlock>,
    /// Header of the genesis (block 0) state.
    pub genesis_block_header: BtHeader,
    /// Pre-state: account address -> account data before any blocks are applied.
    pub pre: BTreeMap<String, BtAccount>,
    /// Post-state: expected account states after all blocks are applied (if provided inline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_state: Option<BTreeMap<String, BtAccount>>,
    /// Hash of the expected post-state root (alternative to inline `post_state`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_state_hash: Option<String>,
    /// Hash of the last valid block after execution.
    pub lastblockhash: String,
    /// Target fork/network rule-set name (e.g. "Cancun", "Prague").
    pub network: String,
    /// Consensus engine identifier, if not the default.
    #[serde(default)]
    pub seal_engine: Option<String>,
}

/// A block entry in the test.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtBlock {
    /// Parsed block header; `None` for blocks expected to be invalid.
    pub block_header: Option<BtHeader>,
    /// Hex-encoded RLP of the full block (header + transactions + uncles).
    pub rlp: String,
    /// Uncle/ommer block headers, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncle_headers: Option<Vec<serde_json::Value>>,
    /// If set, this block is expected to be rejected with the given error string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exception: Option<String>,
    /// Present on exception blocks — contains the decoded block header/txs
    /// since the top-level blockHeader is absent for invalid blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rlp_decoded: Option<BtRlpDecoded>,
    /// Pre-parsed transactions (always present in modern test fixtures).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<BtTransaction>>,
    /// Beacon-chain withdrawals included in this block (post-Shanghai).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawals: Option<Vec<serde_json::Value>>,
    /// Hex string of the block number for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocknumber: Option<String>,
}

/// A transaction in blocktest JSON format.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtTransaction {
    /// EIP-2718 transaction type (0x0 legacy, 0x1 access-list, 0x2 dynamic-fee, 0x3 blob, 0x4 EIP-7702).
    #[serde(rename = "type", default)]
    pub tx_type: Option<String>,
    /// Chain ID this transaction targets (hex).
    #[serde(default)]
    pub chain_id: Option<String>,
    /// Sender's nonce (hex).
    pub nonce: String,
    /// Gas price in wei (hex); used by legacy and EIP-2930 transactions.
    #[serde(default)]
    pub gas_price: Option<String>,
    /// Maximum gas units this transaction may consume (hex).
    pub gas_limit: String,
    /// Recipient address; `None` for contract-creation transactions.
    #[serde(default)]
    pub to: Option<String>,
    /// Wei value transferred (hex).
    pub value: String,
    /// Input / calldata (hex).
    pub data: String,
    /// ECDSA recovery id (hex).
    pub v: String,
    /// ECDSA signature r component (hex).
    pub r: String,
    /// ECDSA signature s component (hex).
    pub s: String,
    /// Recovered sender address, if included by the test generator.
    #[serde(default)]
    pub sender: Option<String>,
    /// EIP-1559 miner tip cap in wei (hex).
    #[serde(default)]
    pub max_priority_fee_per_gas: Option<String>,
    /// EIP-1559 max fee per gas in wei (hex).
    #[serde(default)]
    pub max_fee_per_gas: Option<String>,
    /// EIP-4844 max fee per blob gas in wei (hex).
    #[serde(default)]
    pub max_fee_per_blob_gas: Option<String>,
    /// EIP-4844 versioned blob hashes committed to by this transaction.
    #[serde(default)]
    pub blob_versioned_hashes: Option<Vec<String>>,
    /// EIP-2930 access list of (address, storage-keys) pairs.
    #[serde(default)]
    pub access_list: Option<Vec<BtAccessListEntry>>,
    /// EIP-7702 authorization tuples for setting account code.
    #[serde(default)]
    pub authorization_list: Option<Vec<BtAuthorization>>,
}

/// Access list entry.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtAccessListEntry {
    /// Contract address to pre-warm.
    pub address: String,
    /// Storage slot keys to pre-warm for this address.
    pub storage_keys: Vec<String>,
}

/// EIP-7702 authorization tuple.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtAuthorization {
    /// Chain ID this authorization is valid on (hex).
    pub chain_id: String,
    /// Target code address to delegate to.
    pub address: String,
    /// Authorizer's nonce at time of signing (hex).
    pub nonce: String,
    /// ECDSA recovery id (hex).
    pub v: String,
    /// ECDSA signature r component (hex).
    pub r: String,
    /// ECDSA signature s component (hex).
    pub s: String,
    /// Recovered signer address, if included by the test generator.
    #[serde(default)]
    pub signer: Option<String>,
    /// Parity bit of the public key's Y coordinate (alternative to `v`).
    #[serde(default, rename = "yParity")]
    pub y_parity: Option<String>,
}

/// The `rlp_decoded` field on exception blocks.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtRlpDecoded {
    /// Decoded block header from the RLP payload.
    pub block_header: Option<BtHeader>,
}

/// Block/genesis header fields.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtHeader {
    /// Logs bloom filter (hex, 256 bytes).
    pub bloom: String,
    /// Address of the block's fee recipient / miner.
    pub coinbase: String,
    /// Mix hash used by PoW; repurposed as RANDAO prevRandao under PoS.
    pub mix_hash: String,
    /// PoW nonce (hex, 8 bytes); zero-filled under PoS.
    pub nonce: String,
    /// Block number (hex).
    pub number: String,
    /// Keccak-256 hash of this block's header.
    pub hash: String,
    /// Hash of the parent block's header.
    pub parent_hash: String,
    /// Root hash of the receipts trie.
    pub receipt_trie: String,
    /// Root hash of the world state trie after this block.
    pub state_root: String,
    /// Root hash of the transactions trie for this block.
    pub transactions_trie: String,
    /// Hash of the uncle/ommer list.
    pub uncle_hash: String,
    /// Arbitrary extra data included by the block producer (hex).
    pub extra_data: String,
    /// Block difficulty (hex); zero under PoS.
    pub difficulty: String,
    /// Block gas limit (hex).
    pub gas_limit: String,
    /// Total gas consumed by all transactions in this block (hex).
    pub gas_used: String,
    /// Unix timestamp of the block (hex).
    pub timestamp: String,
    /// EIP-1559 base fee per gas (hex); present from London fork onward.
    #[serde(default)]
    pub base_fee_per_gas: Option<String>,
    /// Root hash of the withdrawals trie (post-Shanghai).
    #[serde(default)]
    pub withdrawals_root: Option<String>,
    /// Total blob gas consumed in this block (hex, post-Cancun).
    #[serde(default)]
    pub blob_gas_used: Option<String>,
    /// Excess blob gas carried forward for fee calculation (hex, post-Cancun).
    #[serde(default)]
    pub excess_blob_gas: Option<String>,
    /// Beacon chain parent block root (post-Cancun, EIP-4788).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_beacon_block_root: Option<String>,
    /// Hash of the execution-layer requests in this block (post-Prague, EIP-7685).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_hash: Option<String>,
}

/// Account in pre/post state.
#[derive(Debug, Deserialize, Serialize)]
pub struct BtAccount {
    /// Account balance in wei (hex).
    pub balance: String,
    /// Account nonce / transaction count (hex).
    pub nonce: String,
    /// Deployed bytecode (hex); "0x" for EOAs.
    pub code: String,
    /// Storage slot -> value mapping (both hex).
    pub storage: BTreeMap<String, String>,
}
