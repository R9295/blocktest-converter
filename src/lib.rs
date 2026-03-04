//! Converts a `SimplifiedInput` (minimal test format) into a valid blockchain
//! test JSON (`BlockTestFile`), mirroring the Go `evm convert` command.
//!
//! Pipeline: parse minimal → build genesis → sign txs → execute blocks via
//! reth EVM → compute state roots → assemble blocktest JSON.

mod blocktest;
mod error;
mod minimal;

use std::collections::BTreeMap;

use crate::error::Error;
use alloy_consensus::{
    SignableTransaction, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy, TxReceipt,
};
use alloy_eips::eip2930::{AccessList, AccessListItem};
use alloy_eips::eip4895::Withdrawal;
use alloy_eips::eip7702::Authorization;
use alloy_genesis::GenesisAccount;
use alloy_primitives::{Address, Bloom, Bytes, FixedBytes, B256, U256};
use alloy_rlp::Encodable;
use ef_tests::models::{ForkSpec, Header};
use reth_chainspec::EthChainSpec;
use reth_db_common::init::{insert_genesis_hashes, insert_genesis_history, insert_genesis_state};
use reth_ethereum_primitives::{Block, BlockBody, Transaction, TransactionSigned};
use reth_evm::execute::Executor;
use reth_evm::ConfigureEvm;
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives_traits::crypto::secp256k1::sign_message;
use reth_primitives_traits::{Header as ConsensusHeader, RecoveredBlock};
use reth_primitives_traits::{SealedBlock, SealedHeader, SignerRecoverable};
use reth_provider::test_utils::create_test_provider_factory_with_chain_spec;
use reth_provider::DatabaseProviderFactory;
use reth_provider::StaticFileProviderFactory;
use reth_provider::StaticFileSegment;
use reth_provider::StaticFileWriter;
use reth_provider::StorageSettingsCache;
use reth_provider::{
    BlockWriter, ExecutionOutcome, HistoryWriter, OriginalValuesKnown, StateWriteConfig,
    StateWriter, TrieWriter,
};
use reth_revm::database::StateProviderDatabase;
use reth_trie::{HashedPostState, KeccakKeyHasher, StateRoot};
use reth_trie_db::DatabaseStateRoot;

use crate::blocktest::{BlockTest, BlockTestFile, BtAccount, BtBlock, BtHeader};
use crate::minimal::{MinimalBlock, MinimalTx, SimplifiedInput};

// ---------------------------------------------------------------------------
// Hex parsing helpers
// ---------------------------------------------------------------------------

/// Strip the `0x` or `0X` prefix from a hex string, returning the bare hex digits.
fn strip_hex_prefix(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

/// Parse a hex string into a [`U256`]. Empty or missing digits are treated as zero.
fn parse_u256(value: &str, field: &str) -> Result<U256, Error> {
    let hex = strip_hex_prefix(value);
    let hex = if hex.is_empty() { "0" } else { hex };
    U256::from_str_radix(hex, 16)
        .map_err(|e| Error::ParseError(format!("invalid {field}: {value} ({e})")))
}

/// Parse a hex string into a `u64`, returning an error on overflow.
fn parse_u64(value: &str, field: &str) -> Result<u64, Error> {
    let v = parse_u256(value, field)?;
    u64::try_from(v).map_err(|e| Error::ParseError(format!("{field} overflows u64: {e}")))
}

/// Parse a hex string into a `u128`, returning an error on overflow.
fn parse_u128(value: &str, field: &str) -> Result<u128, Error> {
    let v = parse_u256(value, field)?;
    u128::try_from(v).map_err(|e| Error::ParseError(format!("{field} overflows u128: {e}")))
}

/// Parse a hex string into a 20-byte [`Address`].
fn parse_address(value: &str, field: &str) -> Result<Address, Error> {
    let bytes = hex::decode(strip_hex_prefix(value))
        .map_err(|e| Error::ParseError(format!("invalid {field}: {value} ({e})")))?;
    if bytes.len() != 20 {
        return Err(Error::ParseError(format!(
            "invalid {field}: expected 20 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(Address::from_slice(&bytes))
}

/// Parse a hex string into a 32-byte [`B256`] hash.
fn parse_b256(value: &str, field: &str) -> Result<B256, Error> {
    let bytes = hex::decode(strip_hex_prefix(value))
        .map_err(|e| Error::ParseError(format!("invalid {field}: {value} ({e})")))?;
    if bytes.len() != 32 {
        return Err(Error::ParseError(format!(
            "invalid {field}: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(B256::from_slice(&bytes))
}

/// Parse a hex string into arbitrary [`Bytes`], returning empty bytes on decode failure.
fn parse_bytes(value: &str) -> Bytes {
    let raw = hex::decode(strip_hex_prefix(value)).unwrap_or_default();
    Bytes::from(raw)
}

/// Format a u64 as a minimal hex string (e.g. "0x1a").
fn hex_u64(v: u64) -> String {
    format!("{v:#x}")
}

/// Format a U256 as a minimal hex string.
fn hex_u256(v: U256) -> String {
    format!("{v:#x}")
}

/// Format a U256 as minimal, even-length hex bytes (e.g. 0x01, 0x0a, 0xdeadbeef).
fn hex_u256_even(v: U256) -> String {
    let mut raw = format!("{v:x}");
    if raw.len() % 2 == 1 {
        raw.insert(0, '0');
    }
    format!("0x{raw}")
}

/// Format bytes as hex.
fn hex_bytes(b: &[u8]) -> String {
    if b.is_empty() {
        "0x".to_string()
    } else {
        format!("0x{}", hex::encode(b))
    }
}

/// Format a B256 hash with minimal leading-zero trimming.
fn hex_b256(h: B256) -> String {
    format!("{h:#x}")
}

/// Convert an ef-tests [`Header`] (U256 fields) into a reth [`ConsensusHeader`] (u64 fields).
fn to_consensus_header(header: &Header) -> Result<ConsensusHeader, Error> {
    Ok(ConsensusHeader {
        base_fee_per_gas: header
            .base_fee_per_gas
            .map(u64::try_from)
            .transpose()
            .map_err(|e| Error::ParseError(format!("genesis baseFeePerGas overflows u64: {e}")))?,
        beneficiary: header.coinbase,
        difficulty: header.difficulty,
        extra_data: header.extra_data.clone(),
        gas_limit: u64::try_from(header.gas_limit)
            .map_err(|e| Error::ParseError(format!("genesis overflows u64: {e}")))?,
        gas_used: u64::try_from(header.gas_used)
            .map_err(|e| Error::ParseError(format!("genesis gasUsed overflows u64: {e}")))?,
        mix_hash: header.mix_hash,
        nonce: u64::from_be_bytes(header.nonce.0).into(),
        number: u64::try_from(header.number)
            .map_err(|e| Error::ParseError(format!("genesis number overflows u64: {e}")))?,
        timestamp: u64::try_from(header.timestamp)
            .map_err(|e| Error::ParseError(format!("genesis timestamp overflows u64: {e}")))?,
        transactions_root: header.transactions_trie,
        receipts_root: header.receipt_trie,
        ommers_hash: header.uncle_hash,
        state_root: header.state_root,
        parent_hash: header.parent_hash,
        logs_bloom: header.bloom,
        withdrawals_root: header.withdrawals_root,
        blob_gas_used: header
            .blob_gas_used
            .map(u64::try_from)
            .transpose()
            .map_err(|e| Error::ParseError(format!("genesis blobGasUsed overflows u64: {e}")))?,
        excess_blob_gas: header
            .excess_blob_gas
            .map(u64::try_from)
            .transpose()
            .map_err(|e| Error::ParseError(format!("genesis excessBlobGas overflows u64: {e}")))?,
        parent_beacon_block_root: header.parent_beacon_block_root,
        requests_hash: header.requests_hash,
    })
}

/// Convert a reth [`ConsensusHeader`] back into an ef-tests [`Header`].
fn from_consensus_header(header: &ConsensusHeader) -> Header {
    Header {
        bloom: header.logs_bloom,
        coinbase: header.beneficiary,
        difficulty: header.difficulty,
        extra_data: header.extra_data.clone(),
        gas_limit: U256::from(header.gas_limit),
        gas_used: U256::from(header.gas_used),
        hash: SealedHeader::seal_slow(header.clone()).hash(),
        mix_hash: header.mix_hash,
        nonce: header.nonce,
        number: U256::from(header.number),
        parent_hash: header.parent_hash,
        receipt_trie: header.receipts_root,
        state_root: header.state_root,
        timestamp: U256::from(header.timestamp),
        transactions_trie: header.transactions_root,
        uncle_hash: header.ommers_hash,
        base_fee_per_gas: header.base_fee_per_gas.map(U256::from),
        withdrawals_root: header.withdrawals_root,
        blob_gas_used: header.blob_gas_used.map(U256::from),
        excess_blob_gas: header.excess_blob_gas.map(U256::from),
        parent_beacon_block_root: header.parent_beacon_block_root,
        requests_hash: header.requests_hash,
        target_blobs_per_block: None,
    }
}

/// Convert a reth consensus header to blocktest JSON header.
fn header_to_bt(header: &ConsensusHeader) -> BtHeader {
    let sealed = SealedHeader::seal_slow(header.clone());
    BtHeader {
        bloom: format!("{:#x}", header.logs_bloom),
        coinbase: format!("{:#x}", header.beneficiary),
        mix_hash: hex_b256(header.mix_hash),
        nonce: format!("0x{:016x}", u64::from(header.nonce)),
        number: hex_u64(header.number),
        hash: hex_b256(sealed.hash()),
        parent_hash: hex_b256(header.parent_hash),
        receipt_trie: hex_b256(header.receipts_root),
        state_root: hex_b256(header.state_root),
        transactions_trie: hex_b256(header.transactions_root),
        uncle_hash: hex_b256(header.ommers_hash),
        extra_data: hex_bytes(&header.extra_data),
        difficulty: hex_u256(header.difficulty),
        gas_limit: hex_u64(header.gas_limit),
        gas_used: hex_u64(header.gas_used),
        timestamp: hex_u64(header.timestamp),
        base_fee_per_gas: header.base_fee_per_gas.map(hex_u64),
        withdrawals_root: header.withdrawals_root.map(hex_b256),
        blob_gas_used: header.blob_gas_used.map(hex_u64),
        excess_blob_gas: header.excess_blob_gas.map(hex_u64),
        parent_beacon_block_root: header.parent_beacon_block_root.map(hex_b256),
        requests_hash: header.requests_hash.map(hex_b256),
    }
}

// ---------------------------------------------------------------------------
// Pre-state construction
// ---------------------------------------------------------------------------

/// Build the genesis state as a map of `GenesisAccount`, ready for insertion.
fn build_pre_state(input: &SimplifiedInput) -> Result<BTreeMap<Address, GenesisAccount>, Error> {
    let mut state = BTreeMap::new();
    for (addr_str, acct) in &input.accounts {
        let addr = parse_address(addr_str, "account address")?;
        let balance = parse_u256(&acct.balance, "balance")?;
        let nonce = parse_u64(&acct.nonce, "nonce")?;
        let code = parse_bytes(acct.code.as_deref().unwrap_or("0x"));
        let mut storage = BTreeMap::new();
        for (k, v) in &acct.storage {
            let key = parse_b256(k, "storage key")?;
            let val = parse_b256(v, "storage value")?;
            if val != B256::ZERO {
                storage.insert(key, val);
            }
        }
        state.insert(
            addr,
            GenesisAccount {
                balance,
                nonce: Some(nonce),
                code: Some(code).filter(|c| !c.is_empty()),
                storage: Some(storage),
                private_key: None,
            },
        );
    }
    Ok(state)
}

// ---------------------------------------------------------------------------
// Transaction construction and signing
// ---------------------------------------------------------------------------

/// Build and sign a single transaction.
#[allow(clippy::too_many_lines)]
fn build_signed_tx(
    input: &SimplifiedInput,
    tx: &MinimalTx,
    base_fee: u64,
) -> Result<TransactionSigned, Error> {

    // Parse common fields
    let nonce = parse_u64(&tx.nonce, "tx nonce")?;
    let gas_limit = parse_u64(&tx.gas, "tx gas")?;
    let value = parse_u256(&tx.value, "tx value")?;
    let tx_chain_id = parse_u64(&tx.chain_id, "tx chainId")?;
    let data = parse_bytes(&tx.data);
    let to = match &tx.to {
        Some(addr_str) => alloy_primitives::TxKind::Call(parse_address(addr_str, "tx to")?),
        None => alloy_primitives::TxKind::Create,
    };
    let access_list_vec: Vec<AccessListItem> = tx
        .access_list
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|entry| {
            let address = parse_address(&entry.address, "tx accessList address")?;
            let storage_keys = entry
                .storage_keys
                .iter()
                .map(|key| parse_b256(key, "tx accessList storage key"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AccessListItem {
                address,
                storage_keys,
            })
        })
        .collect::<Result<_, _>>()?;
    let access_list = AccessList::from(access_list_vec);
    let resolved_tx_type = tx.tx_type;

    let legacy_chain_id = Some(tx_chain_id);

    let typed_chain_id = tx_chain_id;
    let transaction: Transaction = match resolved_tx_type {
        0 => {
            // Legacy
            let gas_price = parse_u128(
                tx.gas_price.as_deref().ok_or_else(|| Error::ConversionFailure("type 0 tx missing gasPrice".to_string()))?,
                "tx gasPrice",
            )?;
            TxLegacy {
                chain_id: legacy_chain_id,
                nonce,
                gas_price,
                gas_limit,
                to,
                value,
                input: data,
            }
            .into()
        }
        1 => {
            // EIP-2930 access list
            let gas_price = parse_u128(
                tx.gas_price.as_deref().ok_or_else(|| Error::ConversionFailure("type 1 tx missing gasPrice".to_string()))?,
                "tx gasPrice",
            )?;
            TxEip2930 {
                chain_id: typed_chain_id,
                nonce,
                gas_price,
                gas_limit,
                to,
                value,
                input: data,
                access_list: access_list.clone(),
            }
            .into()
        }
        2 => {
            // EIP-1559 dynamic fee
            let tip = parse_u128(&tx.max_priority_fee, "maxPriorityFee")?;
            let fee_cap = parse_u128(&tx.max_fee, "maxFee")?;
            TxEip1559 {
                chain_id: typed_chain_id,
                nonce,
                gas_limit,
                max_fee_per_gas: fee_cap,
                max_priority_fee_per_gas: tip,
                to,
                value,
                input: data,
                access_list: access_list.clone(),
            }
            .into()
        }
        3 => {
            // EIP-4844 blob
            let tip = parse_u128(&tx.max_priority_fee, "maxPriorityFee")?;
            let fee_cap = parse_u128(&tx.max_fee, "maxFee")?;
            let blob_fee_cap = tx
                .max_fee_per_blob_gas
                .as_deref()
                .map(|v| parse_u128(v, "maxFeePerBlobGas"))
                .transpose()?
                .unwrap_or(0);
            // Blob tx `to` must be an Address (contract creation not allowed per EIP-4844)
            let to_addr = parse_address(
                tx.to.as_deref().ok_or_else(|| Error::ConversionFailure("type 3 (blob) tx requires a `to` address".to_string()))?,
                "tx to",
            )?;
            let blob_hashes = tx
                .blob_versioned_hashes
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|h| parse_b256(h, "blobVersionedHash"))
                .collect::<Result<Vec<_>, _>>()?;
            TxEip4844 {
                chain_id: typed_chain_id,
                nonce,
                gas_limit,
                max_fee_per_gas: fee_cap,
                max_priority_fee_per_gas: tip,
                to: to_addr,
                value,
                input: data,
                access_list: access_list.clone(),
                blob_versioned_hashes: blob_hashes,
                max_fee_per_blob_gas: blob_fee_cap,
            }
            .into()
        }
        4 => {
            // EIP-7702 set-code
            let tip = parse_u128(&tx.max_priority_fee, "maxPriorityFee")?;
            let fee_cap = parse_u128(&tx.max_fee, "maxFee")?;
            // Set-code tx `to` must be an Address (contract creation not allowed per EIP-7702)
            let to_addr = parse_address(
                tx.to.as_deref().ok_or_else(|| Error::ConversionFailure("type 4 (set-code) tx requires a `to` address".to_string()))?,
                "tx to",
            )?;
            let auth_list = tx
                .authorization_list
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|a| {
                    let auth_chain_id = parse_u256(&a.chain_id, "auth chainId")?;
                    let address = parse_address(&a.address, "auth address")?;
                    let auth_nonce = parse_u64(&a.nonce, "auth nonce")?;
                    let auth = Authorization {
                        chain_id: auth_chain_id,
                        address,
                        nonce: auth_nonce,
                    };
                    let signer_acct = input
                        .accounts
                        .get(&a.signer)
                        .ok_or_else(|| Error::ConversionFailure(format!("auth signer {} not found", a.signer)))?;
                    let signer_pk_hex = signer_acct
                        .private_key
                        .as_deref()
                        .ok_or_else(|| Error::ConversionFailure(format!("auth signer {} missing private key", a.signer)))?;
                    let signer_pk_bytes = hex::decode(strip_hex_prefix(signer_pk_hex))
                        .map_err(|e| Error::ConversionFailure(format!("invalid auth signer private key: {e}")))?;
                    if signer_pk_bytes.len() != 32 {
                        return Err(Error::ConversionFailure(format!(
                            "auth signer {} private key must be 32 bytes, got {}",
                            a.signer,
                            signer_pk_bytes.len()
                        )));
                    }
                    let signer_pk = B256::from_slice(&signer_pk_bytes);
                    let sig = sign_message(signer_pk, auth.signature_hash())
                        .map_err(|e| Error::ConversionFailure(format!("auth signing failed: {e}")))?;
                    Ok(auth.into_signed(sig))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            TxEip7702 {
                chain_id: typed_chain_id,
                nonce,
                gas_limit,
                max_fee_per_gas: fee_cap,
                max_priority_fee_per_gas: tip,
                to: to_addr,
                value,
                input: data,
                access_list: access_list.clone(),
                authorization_list: auth_list,
            }
            .into()
        }
        _ => return Err(Error::ConversionFailure(format!("unsupported tx type {resolved_tx_type}"))),
    };

    let sender_acct = input
        .accounts
        .get(&tx.from)
        .ok_or_else(|| Error::ConversionFailure(format!("sender {} not found", tx.from)))?;
    let pk_hex = sender_acct
        .private_key
        .as_deref()
        .ok_or_else(|| Error::ConversionFailure(format!("sender {} missing private key", tx.from)))?;
    let pk_bytes = hex::decode(strip_hex_prefix(pk_hex))
        .map_err(|e| Error::ConversionFailure(format!("invalid private key: {e}")))?;
    if pk_bytes.len() != 32 {
        return Err(Error::ConversionFailure(format!(
            "private key for {} must be 32 bytes, got {}",
            tx.from,
            pk_bytes.len()
        )));
    }
    let private_key = B256::from_slice(&pk_bytes);
    let sig_hash = transaction.signature_hash();
    let signature = sign_message(private_key, sig_hash)
        .map_err(|e| Error::ConversionFailure(format!("signing failed: {e}")))?;
    let signed: TransactionSigned = transaction.into_signed(signature).into();

    Ok(signed)
}

// ---------------------------------------------------------------------------
// Block execution result
// ---------------------------------------------------------------------------

/// The result of processing a single block: header, body, and optional exception.
#[allow(dead_code)]
struct BlockResult {
    header: ConsensusHeader,
    body: BlockBody,
    senders: Vec<Address>,
    expect_exception: Option<String>,
}

/// Partial execution state recovered from transactions that succeeded before
/// an invalid transaction caused the block to fail. Used to populate exception
/// block headers with realistic roots and gas values.
#[derive(Debug, Clone, Copy)]
struct ExceptionExecutionFields {
    state_root: B256,
    receipts_root: B256,
    logs_bloom: Bloom,
    gas_used: u64,
    blob_gas_used: u64,
    requests_hash: B256,
}

// ---------------------------------------------------------------------------
// Public conversion entry point
// ---------------------------------------------------------------------------

/// Convert a `SimplifiedInput` into a `BlockTestFile`.
///
/// Builds genesis, signs txs,
/// executes blocks through reth's EVM, computes state roots, and assembles
/// the blocktest JSON output.
#[allow(clippy::too_many_lines)]
pub fn convert(input: &SimplifiedInput) -> Result<BlockTestFile, Error> {
    if input.blocks.is_empty() {
        return Err(Error::ConversionFailure("input has no blocks".to_string()));
    }
    let fork_spec: ForkSpec = serde_json::from_value(serde_json::Value::String(input.fork.clone()))
        .map_err(|_| Error::ConversionFailure(format!("unsupported fork: {}", input.fork)))?;
    let chain_spec = fork_spec.to_chain_spec();
    // --- Build genesis header from chain spec defaults + env overrides ---
    let mut genesis_ef_header = from_consensus_header(chain_spec.genesis_header());
    apply_env_to_genesis(&mut genesis_ef_header, input)?;
    genesis_ef_header.hash =
        SealedHeader::seal_slow(to_consensus_header(&genesis_ef_header)?).hash();

    // --- Build pre-state and insert into provider ---
    let pre_state = build_pre_state(input)?;
    let factory = create_test_provider_factory_with_chain_spec(chain_spec.clone());
    let provider = factory
        .database_provider_rw()
        .map_err(|e| Error::provider_error(e))?;

    // Insert genesis block
    let db_genesis_block = SealedBlock::<Block>::from_sealed_parts(
        genesis_ef_header.clone().into(),
        BlockBody::default(),
    )
    .try_recover()
    .map_err(|e| Error::provider_error(e))?;
    provider
        .insert_block(&db_genesis_block)
        .map_err(|e| Error::provider_error(e))?;
    provider
        .static_file_provider()
        .latest_writer(StaticFileSegment::Receipts)
        .and_then(|mut w| w.increment_block(0))
        .map_err(|e| Error::provider_error(e))?;

    // Insert genesis state
    insert_genesis_state(&provider, pre_state.iter()).map_err(|e| Error::provider_error(e))?;
    insert_genesis_hashes(&provider, pre_state.iter()).map_err(|e| Error::provider_error(e))?;
    insert_genesis_history(&provider, pre_state.iter()).map_err(|e| Error::provider_error(e))?;
    // Compute genesis state root from the trie
    let (genesis_state_root, trie_updates) = reth_trie_db::with_adapter!(provider, |A| {
        StateRoot::<
            reth_trie_db::DatabaseTrieCursorFactory<_, A>,
            reth_trie_db::DatabaseHashedCursorFactory<_>,
        >::from_tx(provider.tx_ref())
        .root_with_updates()
    })
    .map_err(|e| Error::provider_error(e))?;
    provider
        .write_trie_updates(trie_updates)
        .map_err(|e| Error::provider_error(e))?;

    // Update genesis header with computed state root
    genesis_ef_header.state_root = genesis_state_root;
    genesis_ef_header.hash =
        SealedHeader::seal_slow(to_consensus_header(&genesis_ef_header)?).hash();

    let genesis_consensus = to_consensus_header(&genesis_ef_header)?;
    let genesis_bt_header = header_to_bt(&genesis_consensus);

    // --- Parse block-level environment (coinbase, timestamp, random, gas_limit) ---
    let block_env = parse_block_env(input)?;

    // --- Execute blocks ---
    let executor_provider = EthEvmConfig::ethereum(chain_spec.clone());
    let mut parent_header = genesis_consensus.clone();

    let blocks = input.blocks.clone();

    let mut block_results: Vec<BlockResult> = Vec::new();
    let mut next_block_number: u64 = 1;

    for input_block in &blocks {
        // Always use sequential numbering — the static file provider requires
        // blocks to be inserted in order with no gaps.
        let block_number = next_block_number;
        let block_timestamp = if let Some(ref ts) = input_block.timestamp {
            parse_u64(ts, "block timestamp")?
        } else if block_number == 1 {
            // First derived block anchors to env timestamp.
            block_env.base_timestamp
        } else {
            // For mixed explicit/derived timestamp streams, derive from parent
            // to keep monotonicity and avoid `timestamp <= parent.timestamp`.
            parent_header.timestamp.saturating_add(12)
        };

        // Compute base fee: per-block override or derived from parent
        let block_base_fee = if let Some(ref bf) = input_block.base_fee_per_gas {
            parse_u64(bf, "block baseFeePerGas")?
        } else {
            chain_spec
                .next_block_base_fee(&parent_header, block_timestamp)
                .unwrap_or(parent_header.base_fee_per_gas.unwrap_or(0))
        };

        // Per-block coinbase override
        let block_coinbase = if let Some(ref cb) = input_block.coinbase {
            parse_address(cb, "block coinbase")?
        } else {
            block_env.coinbase
        };

        // Per-block difficulty override
        let block_difficulty = if let Some(ref d) = input_block.difficulty {
            parse_u256(d, "block difficulty")?
        } else {
            U256::ZERO
        };

        // Compute excess blob gas from parent (fork-aware via BlobParams)
        let parent_excess = parent_header.excess_blob_gas.unwrap_or(0);
        let parent_blob_used = parent_header.blob_gas_used.unwrap_or(0);
        let parent_base_fee_val = parent_header.base_fee_per_gas.unwrap_or(0);
        let excess_blob_gas = if let Some(ref ebg) = input_block.excess_blob_gas {
            parse_u64(ebg, "block excessBlobGas")?
        } else {
            chain_spec
                .blob_params_at_timestamp(block_timestamp)
                .map_or_else(
                    || alloy_eips::eip4844::calc_excess_blob_gas(parent_excess, parent_blob_used),
                    |bp| {
                        bp.next_block_excess_blob_gas_osaka(
                            parent_excess,
                            parent_blob_used,
                            parent_base_fee_val,
                        )
                    },
                )
        };

        // Sign all transactions for this block
        let mut signed_txs = Vec::new();
        let mut senders = Vec::new();
        let mut build_err = None;

        for (tx_idx, tx) in input_block.transactions.iter().enumerate() {
            match build_signed_tx(input, tx, block_base_fee) {
                Ok(signed) => {
                    let sender = signed.recover_signer().map_err(|e| {
                        Error::ConversionFailure(format!(
                            "block {block_number} tx {tx_idx}: sender recovery failed: {e}"
                        ))
                    })?;
                    senders.push(sender);
                    signed_txs.push(signed);
                }
                Err(e) => {
                    build_err = Some(e);
                    break;
                }
            }
        }

        // Parse withdrawals for this block
        let withdrawals: Vec<Withdrawal> = input_block
            .withdrawals
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|w| {
                Ok(Withdrawal {
                    index: parse_u64(&w.index, "withdrawal index")?,
                    validator_index: parse_u64(&w.validator_index, "withdrawal validatorIndex")?,
                    address: parse_address(&w.address, "withdrawal address")?,
                    amount: parse_u64(&w.amount, "withdrawal amount")?,
                })
            })
            .collect::<Result<_, Error>>()?;
        let withdrawals_root = alloy_consensus::proofs::calculate_withdrawals_root(&withdrawals);
        let beacon_root = input_block
            .parent_beacon_block_root
            .as_deref()
            .map(|v| parse_b256(v, "block parentBeaconBlockRoot"))
            .transpose()?
            .unwrap_or(B256::ZERO);

        // If tx building failed, we can't execute; emit an exception block with
        // the txs we could build and a structurally valid header/body.
        if let Some(build_err) = build_err {
            let result = build_exception_block(
                &parent_header,
                block_number,
                block_timestamp,
                block_base_fee,
                excess_blob_gas,
                beacon_root,
                input_block
                    .expect_exception
                    .clone()
                    .or_else(|| Some(build_err.to_string())),
                &block_env,
                block_coinbase,
                block_difficulty,
                signed_txs.clone(),
                withdrawals.clone(),
                None,
            );
            block_results.push(result);
            // Exception blocks don't advance parent state
            continue;
        }

        // Build a preliminary header for EVM execution
        let parent_hash = SealedHeader::seal_slow(parent_header.clone()).hash();

        let prelim_header = ConsensusHeader {
            parent_hash,
            ommers_hash: alloy_consensus::constants::EMPTY_OMMER_ROOT_HASH,
            beneficiary: block_coinbase,
            state_root: B256::ZERO, // placeholder — computed after execution
            transactions_root: B256::ZERO, // placeholder
            receipts_root: B256::ZERO, // placeholder
            logs_bloom: Bloom::default(),
            difficulty: block_difficulty,
            number: block_number,
            gas_limit: block_env.gas_limit,
            gas_used: 0, // placeholder
            timestamp: block_timestamp,
            extra_data: Bytes::default(),
            mix_hash: block_env.random,
            nonce: FixedBytes::default(),
            base_fee_per_gas: Some(block_base_fee),
            withdrawals_root: Some(withdrawals_root),
            blob_gas_used: Some(0), // placeholder
            excess_blob_gas: Some(excess_blob_gas),
            parent_beacon_block_root: Some(beacon_root),
            requests_hash: Some(B256::ZERO), // placeholder
        };

        // Build the block for execution
        let body = BlockBody {
            transactions: signed_txs.clone(),
            ommers: vec![],
            withdrawals: Some(withdrawals.clone().into()),
        };
        let block = Block {
            header: prelim_header.clone(),
            body: body.clone(),
        };
        let recovered = RecoveredBlock::new_unhashed(block, senders.clone());

        // Execute the block through reth's EVM
        let state_provider = provider.latest();
        let state_db = StateProviderDatabase(&state_provider);
        let executor = executor_provider.batch_executor(state_db);

        let output = match executor.execute(&recovered) {
            Ok(output) => output,
            Err(exec_err) => {
                // Try to recover partial execution fields up to the invalid tx so
                // exception headers reflect real gas/root progression when possible.
                let mut partial_fields = None;
                if let Some(invalid_hash) = invalid_tx_hash(&exec_err) {
                    if let Some(invalid_idx) =
                        signed_txs.iter().position(|tx| *tx.hash() == invalid_hash)
                    {
                        let prefix_txs = signed_txs[..invalid_idx].to_vec();
                        let prefix_senders = senders[..invalid_idx].to_vec();

                        let prefix_header = ConsensusHeader {
                            transactions_root: alloy_consensus::proofs::calculate_transaction_root(
                                &prefix_txs,
                            ),
                            ..prelim_header.clone()
                        };
                        let prefix_body = BlockBody {
                            transactions: prefix_txs,
                            ommers: vec![],
                            withdrawals: Some(withdrawals.clone().into()),
                        };
                        let prefix_block = Block {
                            header: prefix_header,
                            body: prefix_body,
                        };
                        let prefix_recovered =
                            RecoveredBlock::new_unhashed(prefix_block, prefix_senders);

                        let prefix_state_provider = provider.latest();
                        let prefix_state_db = StateProviderDatabase(&prefix_state_provider);
                        let prefix_executor = executor_provider.batch_executor(prefix_state_db);

                        if let Ok(prefix_output) = prefix_executor.execute(&prefix_recovered) {
                            let hashed_state = HashedPostState::from_bundle_state::<KeccakKeyHasher>(
                                prefix_output.state.state(),
                            );
                            let sorted = hashed_state.clone_into_sorted();
                            let state_root = reth_trie_db::with_adapter!(provider, |A| {
                                StateRoot::<
                                        reth_trie_db::DatabaseTrieCursorFactory<_, A>,
                                        _,
                                    >::overlay_root_with_updates(provider.tx_ref(), &sorted)
                            })
                            .map(|(root, _)| root)
                            .unwrap_or(parent_header.state_root);

                            let receipts_with_bloom: Vec<_> = prefix_output
                                .receipts
                                .iter()
                                .map(|r| TxReceipt::with_bloom_ref(r))
                                .collect();
                            let receipts_root = alloy_consensus::proofs::calculate_receipt_root(
                                &receipts_with_bloom,
                            );
                            let logs_bloom = receipts_with_bloom
                                .iter()
                                .fold(Bloom::ZERO, |bloom, r| bloom | r.bloom_ref());

                            partial_fields = Some(ExceptionExecutionFields {
                                state_root,
                                receipts_root,
                                logs_bloom,
                                gas_used: prefix_output.gas_used,
                                blob_gas_used: prefix_output.blob_gas_used,
                                requests_hash: prefix_output.requests.requests_hash(),
                            });
                        }
                    }
                }

                // Execution failed — emit exception block without advancing state.
                let result = build_exception_block(
                    &parent_header,
                    block_number,
                    block_timestamp,
                    block_base_fee,
                    excess_blob_gas,
                    beacon_root,
                    input_block
                        .expect_exception
                        .clone()
                        .or_else(|| Some(exec_err.to_string()))
                        .or_else(|| Some("block expected to fail".to_string())),
                    &block_env,
                    block_coinbase,
                    block_difficulty,
                    signed_txs.clone(),
                    withdrawals.clone(),
                    partial_fields,
                );
                block_results.push(result);
                continue;
            }
        };

        // Compute state root from execution output
        let hashed_state =
            HashedPostState::from_bundle_state::<KeccakKeyHasher>(output.state.state());
        let sorted = hashed_state.clone_into_sorted();
        let (computed_state_root, trie_updates) =
            reth_trie_db::with_adapter!(provider, |A| {
                StateRoot::<reth_trie_db::DatabaseTrieCursorFactory<_, A>, _>::overlay_root_with_updates(
                    provider.tx_ref(),
                    &sorted,
                )
            })
            .map_err(|e| Error::provider_error(e))?;

        // Compute transaction and receipt roots + logs bloom
        let tx_root = alloy_consensus::proofs::calculate_transaction_root(&signed_txs);
        let receipts_with_bloom: Vec<_> = output
            .receipts
            .iter()
            .map(|r| TxReceipt::with_bloom_ref(r))
            .collect();
        let receipt_root = alloy_consensus::proofs::calculate_receipt_root(&receipts_with_bloom);
        let logs_bloom = receipts_with_bloom
            .iter()
            .fold(Bloom::ZERO, |bloom, r| bloom | r.bloom_ref());

        // Compute requests hash
        let requests_hash = output.requests.requests_hash();

        // Blob gas used from execution output
        let blob_gas_used = output.blob_gas_used;

        // Build final header with all computed fields
        let final_header = ConsensusHeader {
            state_root: computed_state_root,
            transactions_root: tx_root,
            receipts_root: receipt_root,
            logs_bloom,
            gas_used: output.gas_used,
            blob_gas_used: Some(blob_gas_used),
            requests_hash: Some(requests_hash),
            ..prelim_header
        };

        // Insert the block first — write_state and update_history_indices
        // need block body indices to already exist in the DB.
        let final_block = Block {
            header: final_header.clone(),
            body: body.clone(),
        };
        let final_recovered = RecoveredBlock::new_unhashed(final_block, senders.clone());
        provider
            .insert_block(&final_recovered)
            .map_err(|e| Error::provider_error(e))?;
        // Commit static files so subsequent provider.latest() sees this block
        provider
            .static_file_provider()
            .commit()
            .map_err(|e| Error::provider_error(e))?;

        // Write execution state to the database (matching ef-tests pattern)
        provider
            .write_state(
                &ExecutionOutcome::single(block_number, output),
                OriginalValuesKnown::Yes,
                StateWriteConfig::default(),
            )
            .map_err(|e| Error::provider_error(e))?;
        provider
            .write_hashed_state(&hashed_state.into_sorted())
            .map_err(|e| Error::provider_error(e))?;
        provider
            .write_trie_updates(trie_updates)
            .map_err(|e| Error::provider_error(e))?;
        provider
            .update_history_indices(block_number..=block_number)
            .map_err(|e| Error::provider_error(e))?;

        block_results.push(BlockResult {
            header: final_header.clone(),
            body,
            senders,
            expect_exception: None,
        });

        // Advance parent and block number for next block
        parent_header = final_header;
        next_block_number = block_number + 1;
    }

    // --- Assemble blocktest JSON output ---
    assemble_output(
        input,
        &genesis_bt_header,
        &genesis_consensus,
        &block_results,
        &input.fork,
    )
}

// ---------------------------------------------------------------------------
// Exception block builder (for blocks expected to be invalid)
// ---------------------------------------------------------------------------

/// Build a [`BlockResult`] for a block that is expected to be invalid.
///
/// If `execution_fields` is provided (from partially executing valid prefix
/// transactions), the header reflects the real state/receipt roots up to the
/// point of failure. Otherwise, roots default to the parent's values.
#[allow(clippy::too_many_arguments)]
fn build_exception_block(
    parent: &ConsensusHeader,
    block_number: u64,
    timestamp: u64,
    base_fee: u64,
    excess_blob_gas: u64,
    parent_beacon_block_root: B256,
    expect_exception: Option<String>,
    block_env: &BlockEnv,
    coinbase: Address,
    difficulty: U256,
    transactions: Vec<TransactionSigned>,
    withdrawals: Vec<Withdrawal>,
    execution_fields: Option<ExceptionExecutionFields>,
) -> BlockResult {
    let parent_hash = SealedHeader::seal_slow(parent.clone()).hash();
    let tx_root = alloy_consensus::proofs::calculate_transaction_root(&transactions);
    let withdrawals_root = alloy_consensus::proofs::calculate_withdrawals_root(&withdrawals);
    let state_root = execution_fields
        .as_ref()
        .map_or(parent.state_root, |fields| fields.state_root);
    let receipts_root = execution_fields
        .as_ref()
        .map_or(alloy_consensus::constants::EMPTY_RECEIPTS, |fields| {
            fields.receipts_root
        });
    let logs_bloom = execution_fields
        .as_ref()
        .map_or(Bloom::default(), |fields| fields.logs_bloom);
    let gas_used = execution_fields
        .as_ref()
        .map_or(0, |fields| fields.gas_used);
    let blob_gas_used = execution_fields
        .as_ref()
        .map_or(0, |fields| fields.blob_gas_used);
    let requests_hash = execution_fields
        .as_ref()
        .map_or(alloy_eips::eip7685::EMPTY_REQUESTS_HASH, |fields| {
            fields.requests_hash
        });

    let header = ConsensusHeader {
        parent_hash,
        ommers_hash: alloy_consensus::constants::EMPTY_OMMER_ROOT_HASH,
        beneficiary: coinbase,
        state_root,
        transactions_root: tx_root,
        receipts_root,
        logs_bloom,
        difficulty,
        number: block_number,
        gas_limit: block_env.gas_limit,
        gas_used,
        timestamp,
        extra_data: Bytes::default(),
        mix_hash: block_env.random,
        nonce: FixedBytes::default(),
        base_fee_per_gas: Some(base_fee),
        withdrawals_root: Some(withdrawals_root),
        blob_gas_used: Some(blob_gas_used),
        excess_blob_gas: Some(excess_blob_gas),
        parent_beacon_block_root: Some(parent_beacon_block_root),
        requests_hash: Some(requests_hash),
    };

    let body = BlockBody {
        transactions,
        ommers: vec![],
        withdrawals: Some(withdrawals.into()),
    };

    BlockResult {
        header,
        body,
        senders: vec![],
        expect_exception: Some(
            expect_exception.unwrap_or_else(|| "block expected to fail".to_string()),
        ),
    }
}

/// Extract the transaction hash from a [`BlockExecutionError::InvalidTx`] variant, if present.
fn invalid_tx_hash(err: &reth_evm::execute::BlockExecutionError) -> Option<B256> {
    match err.as_validation() {
        Some(reth_evm::execute::BlockValidationError::InvalidTx { hash, .. }) => Some(*hash),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Apply environment overrides to genesis header
// ---------------------------------------------------------------------------

/// Block-level environment values parsed from `MinimalEnv`.
struct BlockEnv {
    coinbase: Address,
    base_timestamp: u64,
    gas_limit: u64,
    random: B256,
}

/// Apply only genesis-relevant fields from the environment.
/// In Go, genesis always gets timestamp=0, number=0, and only `gas_limit` /
/// `base_fee` / difficulty from env.  The other env fields (coinbase, timestamp,
/// random) are block-level values.
fn apply_env_to_genesis(header: &mut Header, input: &SimplifiedInput) -> Result<(), Error> {
    let env = &input.env;
    let gas_limit = parse_u64(&env.current_gas_limit, "currentGasLimit")?;
    if gas_limit > 0 {
        header.gas_limit = U256::from(gas_limit);
    }
    header.difficulty = parse_u256(&env.current_difficulty, "currentDifficulty")?;
    header.base_fee_per_gas = Some(parse_u256(&env.current_base_fee, "currentBaseFee")?);
    header.excess_blob_gas = Some(
        env.current_excess_blob_gas
            .as_deref()
            .map(|v| parse_u64(v, "currentExcessBlobGas"))
            .transpose()?
            .map(U256::from)
            .unwrap_or(U256::ZERO),
    );
    // Genesis always has timestamp=0 and number=0 (matching Go converter)
    header.timestamp = U256::ZERO;
    header.number = U256::ZERO;
    header.gas_used = U256::ZERO;
    // Execution-spec fixtures use a single zero byte (`0x00`) in genesis extraData.
    // Keeping this byte preserves the genesis hash, which is read by EIP-2935
    // history storage in block 1 and therefore affects state roots.
    header.extra_data = Bytes::from(vec![0u8]);
    header.nonce = FixedBytes::default();
    Ok(())
}

/// Parse block-level environment values (used during block execution, NOT genesis).
fn parse_block_env(input: &SimplifiedInput) -> Result<BlockEnv, Error> {
    let env = &input.env;
    let coinbase = parse_address(&env.current_coinbase, "currentCoinbase")?;
    let ts = parse_u64(&env.current_timestamp, "currentTimestamp")?;
    let base_timestamp = if ts > 0 { ts } else { 0x1000 };
    let gas_limit_val = parse_u64(&env.current_gas_limit, "currentGasLimit")?;
    let gas_limit = if gas_limit_val > 0 {
        gas_limit_val
    } else {
        0x100_0000
    };
    let random = parse_b256(&env.current_random, "currentRandom")?;
    Ok(BlockEnv {
        coinbase,
        base_timestamp,
        gas_limit,
        random,
    })
}

// ---------------------------------------------------------------------------
// Assemble final blocktest JSON
// ---------------------------------------------------------------------------

/// Assemble the final [`BlockTestFile`] JSON from execution results.
///
/// RLP-encodes each block, builds pre-state alloc, and computes the last
/// valid block hash and post-state hash.
fn assemble_output(
    input: &SimplifiedInput,
    genesis_bt_header: &BtHeader,
    genesis_consensus: &ConsensusHeader,
    results: &[BlockResult],
    fork_name: &str,
) -> Result<BlockTestFile, Error> {
    // Pre-state accounts
    let pre = build_pre_alloc(input)?;

    // Build block entries
    let mut bt_blocks = Vec::new();
    let mut last_valid_header = genesis_consensus.clone();

    for result in results {
        // RLP-encode the block
        let block = Block {
            header: result.header.clone(),
            body: result.body.clone(),
        };
        let sealed = SealedBlock::seal_slow(block);
        let mut rlp_buf = Vec::new();
        sealed.encode(&mut rlp_buf);
        let rlp_hex = format!("0x{}", hex::encode(&rlp_buf));

        if result.expect_exception.is_some() {
            // Exception block: no blockHeader, no txs, no withdrawals, null uncles
            bt_blocks.push(BtBlock {
                block_header: None,
                rlp: rlp_hex,
                uncle_headers: None,
                expect_exception: result.expect_exception.clone(),
                rlp_decoded: None,
                transactions: None,
                withdrawals: None,
                blocknumber: None,
            });
        } else {
            // Valid block
            let hdr = header_to_bt(&result.header);
            bt_blocks.push(BtBlock {
                block_header: Some(hdr),
                rlp: rlp_hex,
                uncle_headers: None,
                expect_exception: None,
                rlp_decoded: None,
                transactions: Some(vec![]),
                withdrawals: Some(vec![]),
                blocknumber: None,
            });
            last_valid_header = result.header.clone();
        }
    }

    // Last valid block hash
    let last_hash = SealedHeader::seal_slow(last_valid_header.clone()).hash();
    let post_state_hash = last_valid_header.state_root;

    let test = BlockTest {
        blocks: bt_blocks,
        genesis_block_header: genesis_bt_header.clone(),
        pre,
        post_state: None,
        post_state_hash: Some(hex_b256(post_state_hash)),
        lastblockhash: hex_b256(last_hash),
        network: fork_name.to_string(),
        seal_engine: Some("NoProof".to_string()),
    };

    let mut output = BlockTestFile::new();
    output.insert("ConvertedTest001".to_string(), test);
    Ok(output)
}

/// Build pre-state alloc as `BtAccount` map (for JSON output).
/// Storage keys are zero-padded 32-byte hex; values use canonical minimal hex.
fn build_pre_alloc(input: &SimplifiedInput) -> Result<BTreeMap<String, BtAccount>, Error> {
    let mut result = BTreeMap::new();
    for (addr_str, acct) in &input.accounts {
        let code = acct.code.as_deref().unwrap_or("0x");
        let mut storage = BTreeMap::new();
        for (k, v) in &acct.storage {
            let key = parse_b256(k, "storage key")?;
            // Render storage values as canonical uint256 quantities (no fixed-width padding),
            // matching go-ethereum/besu blocktest expectations.
            let val = parse_u256(v, "storage value")?;
            if val != U256::ZERO {
                storage.insert(format!("{key:#066x}"), hex_u256_even(val));
            }
        }
        result.insert(
            addr_str.clone(),
            BtAccount {
                balance: acct.balance.clone(),
                nonce: acct.nonce.clone(),
                code: code.to_string(),
                storage,
            },
        );
    }
    Ok(result)
}
