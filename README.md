# blocktest-converter
<img align="right" src="evm.jpeg" alt="logo" width="250" heigh="250">
&nbsp; 

blocktest-converter is an Ethereum [BlockTest](https://ethereum-tests.readthedocs.io/en/v6.0.0-beta.1/test_types/blockchain_tests.html) test fixture generator from a fuzzer friendly input structure.
&nbsp;  

&nbsp; 

To see how it works, look at the [Pipeline](#pipeline) section.  
To see the input format, look at the [Input Format](#input-format) section.  
To see an example input, look at the [Example](#example) section.  
&nbsp;  

## Table of contents
- [About](#about)
- [Usage](#usage)
- [Pipeline](#pipeline)
- [Input format](#input-format)
  - [Transaction types](#transaction-types)
  - [Signing](#signing)
- [Supported forks](#supported-forks)
- [Example Input](#example)

# About
It is extremely difficult to generate valid blocktests when fuzzing as all the hash and root computations in the block's header require actual processing of the block. One bad hash or calculation and the block will be rejected immediately, preventing us from testing anything meaningful.
Adding to the difficulty, comes the precise consturction of the test fixture. No surprises, clients have subtle differences in how they parse the test format. This results in false positives or a waste of fuzzer time and compute.

This library aims to solve both problems and to the best of my knowledge, is the only library-based, spec-compliant and documented implementation.

Block tests allow testing of the entire block processing pipeline, from validation, execution to state commitment. It is a powerful primitive for testing the compliance of EL clients.

Fuzzing with this library has already found three novel bugs (Osaka).
- [Besu #1](https://github.com/hyperledger/besu/issues/9840)
- [Besu #2](https://github.com/hyperledger/besu/issues/9868)
- Potential security impact, currently being triaged..

It additionally found two known bugs in Reth (create collision with empty accounts, max nonce overflow) and one known edge case in Nethermind  which is currently untriggerable (if a deposit contract touches an empty account, state roots will differ). These were not submitted but are mentioned since it shows that the converter is able to reach known issues via a fuzzer.

## Usage

### Rust

```rust
use blocktest_converter::convert;

let fuzzer_input = "{...}"; // see the example input fixutre
let blocktest = convert(fuzzer_input)?;
// this is the block test we can run block tests with (eg. evm blocktest ./output.json).
let output = serde_json::to_string_pretty(&blocktest)?;
std::fs::write("/tmp/blocktest.json", &output)?;
```
Then, using the EVM binary from go-ethereum
``` bash
evm blocktest /tmp/blocktest.json
```
### C / FFI

The crate builds a shared library (`libblocktest_converter.so`) with a C API.

```c
#include <stdio.h>
#include <string.h>

// From blocktest_converter.h
typedef struct {
    unsigned char *data;
    unsigned long  len;
    int            is_err;  // 0 = success, 1 = error
} BlocktestResult;

extern BlocktestResult blocktest_convert(const unsigned char *input, unsigned long len);
extern void blocktest_result_free(unsigned char *ptr, unsigned long len);

int main(void) {
    const char *json = "{...}";  // Input JSON
    BlocktestResult r = blocktest_convert((const unsigned char *)json, strlen(json));
    if (r.is_err) {
        fprintf(stderr, "error: %.*s\n", (int)r.len, r.data);
    } else {
        fwrite(r.data, 1, r.len, stdout);
    }
    blocktest_result_free(r.data, r.len);
    return r.is_err;
}
```

Compile with:
```bash
cargo build --release
clang example.c -L target/release -lblocktest_converter -o example
```

## Pipeline

```
Input (JSON)
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

The input is a JSON object matching the `Input` struct. All hex
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
  "blocks": [                                // list of blocks (with list of transactions within)
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

## Example

A complete `Input` that transfers 1 wei from an EOA to a contract
via an EIP-1559 transaction on Osaka:

```json
{
  "version": "1",
  "fork": "Osaka",
  "chainId": 1,
  "env": {
    "currentCoinbase": "0x2adc25665018aa1fe0e6bc666dac8fc2697ff9ba",
    "currentDifficulty": "0x0",
    "currentGasLimit": "0x1000000",
    "currentNumber": "0x1",
    "currentTimestamp": "0x3e8",
    "currentBaseFee": "0x7",
    "currentRandom": "0x0000000000000000000000000000000000000000000000000000000000000000"
  },
  "accounts": {
    "0xa94f5374fce5edbc8e2a8697c15331677e6ebf0b": {
      "balance": "0xde0b6b3a7640000",
      "nonce": "0x0",
      "storage": {},
      "privateKey": "0x45a915e4d060149eb4365960e6a7a45f334393093061116b197e3240065ff2d8"
    },
    "0x1000000000000000000000000000000000000000": {
      "balance": "0x0",
      "nonce": "0x0",
      "code": "0x6001600055",
      "storage": {}
    }
  },
  "blocks": [
    {
      "transactions": [
        {
          "from": "0xa94f5374fce5edbc8e2a8697c15331677e6ebf0b",
          "chainId": "0x1",
          "to": "0x1000000000000000000000000000000000000000",
          "value": "0x1",
          "gas": "0x186a0",
          "nonce": "0x0",
          "data": "0x",
          "txType": 2,
          "maxFee": "0xe",
          "maxPriorityFee": "0x1"
        }
      ]
    }
  ]
}
```
