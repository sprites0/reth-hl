# reth-hl

HyperEVM archive node implementation based on reth's NodeBuilder API.
Heavily inspired by [reth-bsc](https://github.com/loocapro/reth-bsc).

## TODOs

- [ ] Make it compilable
  - [ ] EVM
  - [v] Storage
  - [ ] TX forwarder API
- [ ] Decide whether to include system txs, receipts in block or not
- [ ] Downloader
  - [ ] S3 format (file)
  - [ ] S3 format (AWS API)
  - [ ] hl-node format
