# reth-hl

HyperEVM archive node implementation based on reth's NodeBuilder API.
Heavily inspired by [reth-bsc](https://github.com/loocapro/reth-bsc).

## Installation

Install the `reth-hl` binary:

```sh
$ make install
```

## Usage

### Ingest from S3

Set AWS credentials in `~/.aws/credentials`. To use a non-default profile:

```sh
$ export AWS_PROFILE=default  # optional
$ reth-hl node --s3 --http --ws --ws.port=8545
```

### Ingest from local

#### S3-style directory (backfill):

```sh
$ reth-hl node --ingest-dir=/path/to/evm-blocks ...
```

#### Native hl-node format (realtime, low latency):

```sh
$ reth-hl node --local-ingest-dir=$HOME/hl/data/evm_blocks_and_receipts ...
```

Or if the path is `$HOME/hl/data/evm_blocks_and_receipts` simply:

```sh
$ reth-hl node --local
```

---

**Note:** This is a draft and will be merged with `nanoreth` documentation.
