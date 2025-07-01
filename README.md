# reth-hl

HyperEVM archive node implementation based on reth's NodeBuilder API.
Heavily inspired by [reth-bsc](https://github.com/loocapro/reth-bsc).

## TODOs

- [x] Make it compilable
  - [x] EVM
  - [x] Storage
  - [x] TX forwarder API
- [x] Decide whether to include system txs, receipts in block or not
- [x] Downloader
  - [x] S3 format (file)
  - [x] S3 format (AWS API)
  - [ ] hl-node format
