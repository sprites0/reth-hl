# nanoreth

HyperEVM archive node implementation based on [reth](https://github.com/paradigmxyz/reth).
NodeBuilder API version is heavily inspired by [reth-bsc](https://github.com/loocapro/reth-bsc).

## ⚠️ IMPORTANT: System Transactions Appear as Pseudo Transactions

Deposit transactions from [System Addresses](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/hyperevm/hypercore-less-than-greater-than-hyperevm-transfers#system-addresses) like `0x222..22` / `0x200..xx` to user addresses are intentionally recorded as pseudo transactions.
This change simplifies block explorers, making it easier to track deposit timestamps.
Ensure careful handling when indexing.

To disable this behavior, add --hl-node-compliant to the CLI arguments-this will not show system transactions and their receipts, mimicking hl-node's output.

## Prerequisites

Building nanoreth from source requires Rust and Cargo to be installed:

`$ curl https://sh.rustup.rs -sSf | sh`

## How to run (mainnet)

1) Setup AWS credentials at `~/.aws/credentials` with read S3 access.

2) `$ make install` - this will install the reth-hl binary.

3) Start nanoreth which will begin syncing using the blocks in `~/evm-blocks`:

```sh
reth-hl node --http --http.addr 0.0.0.0 --http.api eth,ots,net,web3 \
  --ws --ws.addr 0.0.0.0 --ws.origins '*' --ws.api eth,ots,net,web3 \
  --ws.port 8545 --http.port 8545 --s3
```

## How to run (mainnet) (with local block sync)

The `--s3` method above fetches blocks from S3, but you can instead source them from your local hl-node.

This will require you to first have a hl-node outputting blocks prior to running the initial s3 sync,
the node will prioritise locally produced blocks with a fallback to s3.
This method will allow you to reduce the need to rely on S3.

This setup adds `--local-ingest-dir=<path>` (or a shortcut: `--local` if using default hl-node path) to ingest blocks from hl-node, and `--ingest-dir` for fallback copy of EVM blocks. `--ingest-dir` can be replaced with `--s3` if you don't want to
periodically run `aws s3 sync` as below.

```sh
# Run your local hl-node (make sure output file buffering is disabled)
# Make sure evm blocks are being produced inside evm_block_and_receipts
$ hl-node run-non-validator --replica-cmds-style recent-actions --serve-eth-rpc --disable-output-file-buffering

# Fetch EVM blocks (Initial sync)
$ aws s3 sync s3://hl-mainnet-evm-blocks/ ~/evm-blocks --request-payer requester # one-time

# Run node (with local-ingest-dir arg)
$ make install
$ reth-hl node --http --http.addr 0.0.0.0 --http.api eth,ots,net,web3 \
    --ws --ws.addr 0.0.0.0 --ws.origins '*' --ws.api eth,ots,net,web3 --ingest-dir ~/evm-blocks --local-ingest-dir <path-to-your-hl-node-evm-blocks-dir> --ws.port 8545
```

## How to run (testnet)

Testnet is supported since block 21304281.

```sh
# Get testnet genesis at block 21304281
$ cd ~
$ git clone https://github.com/sprites0/hl-testnet-genesis
$ zstd --rm -d ~/hl-testnet-genesis/*.zst

# Init node
$ make install
$ reth-hl init-state --without-evm --chain testnet --header ~/hl-testnet-genesis/21304281.rlp \
  --header-hash 0x5b10856d2b1ad241c9bd6136bcc60ef7e8553560ca53995a590db65f809269b4 \
  ~/hl-testnet-genesis/21304281.jsonl --total-difficulty 0 

# Run node
$ reth-hl node --chain testnet --http --http.addr 0.0.0.0 --http.api eth,ots,net,web3 \
    --ws --ws.addr 0.0.0.0 --ws.origins '*' --ws.api eth,ots,net,web3 --ingest-dir ~/evm-blocks --ws.port 8546
```
