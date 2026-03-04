# blocktest-converter

Converts a fuzzer-friendly input structure into Ethereum's [Block Test](https://ethereum-tests.readthedocs.io/en/v6.0.0-beta.1/test_types/blockchain_tests.html) format -- the standard format consumed by geth, besu, Nethermind, reth and other EL clients.

Block tests allow testing of the entire block processing pipeline, from validation, execution to state commitment.
It is a powerful primitive for testing the adherence of EL clients to a specification.

This library does a best effort at generating valid blocks. Thus, if you wish to test block validation primitives, you'd need to modify the resulting block test.

Fuzzing with this library has already found three novel bugs.
- [Besu #1](https://github.com/hyperledger/besu/issues/9840)
- [Besu #2](https://github.com/hyperledger/besu/issues/9868)
- Potential security impact, currently being triaged..

It additionally found two known bugs in Reth and one known edge case in Nethermind.

To see how it works, look at the Pipeline section below. 

## Usage
This is a Rust library. Call `convert()` from your Rust code:

```rust
use blocktest_converter::{convert, minimal::SimplifiedInput};

let input: SimplifiedInput = serde_json::from_str(&json_string)?;
let blocktest = convert(&input)?;
let output = serde_json::to_string_pretty(&blocktest)?;
```

## Pipeline

```
SimplifiedInput (JSON)
  │
  ├─ 1. Parse fork → select chain spec (Frontier … Osaka)
  ├─ 2. Build genesis header & pre-state
  ├─ 3. Insert genesis into reth provider DB
  │
  ├─ For each block:
  │   ├─ Sign transactions using sender private keys
  │   ├─ Execute block through reth EVM
  │   ├─ Compute state root, tx root, receipt root, logs bloom
  │   └─ Handle exception (invalid) blocks gracefully
  │
  └─ 4. Assemble output as BlockTestFile JSON
```

## Input format

The input is a JSON object matching the `SimplifiedInput` struct. All hex
values use `0x`-prefixed strings.

```jsonc
{
  "version": "1",
  "fork": "Osaka",           // Ethereum fork name (see Supported Forks)
  "chainId": 1,              // integer chain ID
  "env": {
    "currentCoinbase":    "0x...",  // 20-byte address
    "currentDifficulty":  "0x0",
    "currentGasLimit":    "0x1000000",
    "currentNumber":      "0x1",
    "currentTimestamp":   "0x1000",
    "currentBaseFee":     "0x7",
    "currentRandom":      "0x0000...0000",  // 32 bytes
    "currentExcessBlobGas": "0x0"           // optional
  },
  "accounts": {
    "0xSENDER": {
      "balance": "0xde0b6b3a7640000",
      "nonce":   "0x0",
      "code":    "0x",                      // optional, omit for EOAs
      "storage": {},                        // slot → value mapping
      "privateKey": "0xabcdef..."           // optional, required for signers or senders
    },
    "0xCONTRACT": {
      "balance": "0x0",
      "nonce":   "0x1",
      "code":    "0x6000600055",
      "storage": { "0x00..00": "0x00..01" }
    }
  },
  "blocks": [
    {
      "transactions": [
        {
          "from":     "0xSENDER",
          "chainId":  "0x1",
          "to":       "0xCONTRACT",          // null/omit for CREATE
          "value":    "0x0",
          "gas":      "0x5208",
          "nonce":    "0x0",
          "data":     "0x",
          "txType":   0,                      // 0-4, see below

          // Type 0/1 only:
          "gasPrice": "0x7",                  // required for type 0/1

          // Type 2/3/4:
          "maxFee":          "0xa",           // always required
          "maxPriorityFee":  "0x1",           // always required

          // Type 1/2/3/4:
          "accessList": [                     // optional
            { "address": "0x...", "storageKeys": ["0x..."] }
          ],

          // Type 3 only:
          "maxFeePerBlobGas":    "0x1",       // optional
          "blobVersionedHashes": ["0x01..."], // optional

          // Type 4 only:
          "authorizationList": [              // optional
            {
              "chainId": "0x1",
              "address": "0xDELEGATE",
              "nonce":   "0x0",
              "signer":  "0xSIGNER_ADDR"     // must exist in accounts with privateKey
            }
          ]
        }
      ],

      // Optional per-block overrides:
      "withdrawals":          [...],          // EIP-4895 withdrawals
      "expectException":      "SomeError",    // marks block as expected-invalid
      "coinbase":             "0x...",
      "difficulty":           "0x0",
      "number":               "0x1",
      "timestamp":            "0x1000",
      "baseFeePerGas":        "0x7",
      "excessBlobGas":        "0x0",
      "parentBeaconBlockRoot":"0x00..00"
    }
  ]
}
```

### Transaction types

| `txType` | Name | Required fields |
|----------|------|-----------------|
| 0 | Legacy | `gasPrice` |
| 1 | EIP-2930 (access list) | `gasPrice` |
| 2 | EIP-1559 (dynamic fee) | `maxFee`, `maxPriorityFee` |
| 3 | EIP-4844 (blob) | `maxFee`, `maxPriorityFee`, `to` (mandatory) |
| 4 | EIP-7702 (set-code) | `maxFee`, `maxPriorityFee`, `to` (mandatory) |

`maxFee` and `maxPriorityFee` are always present in the struct but only used by
type 2+. `gasPrice` is optional and only used by type 0/1.

### Signing

Transactions are signed automatically using the sender's `privateKey` from the
accounts map. EIP-7702 authorizations are signed using the `signer` field,
which references an account address that must have a `privateKey`.

## Supported forks

Frontier, Homestead, EIP150, EIP158, Byzantium, Constantinople,
ConstantinopleFix, Istanbul, Berlin, London, Merge (Paris), Shanghai, Cancun,
Prague, Osaka.

Transition forks (e.g. `BerlinToLondonAt5`, `ShanghaiToCancunAtTime15k`) are
also supported.
