# note-miner

Small solo miner for the Note chain (scrypt 1024/1/1, CPU only). It asks the
project's own node for work (`getblocktemplate`), assembles the block, searches
a nonce and hands the block back (`submitblock`). Reward, difficulty and
transactions come from the node, so the node's rules (0.0001 NOTE subsidy)
apply unchanged.

Used to revive the chain from the operator's own computer, because the server
CPU only manages about 4 kH/s. Access is set up with the ops task
`note-miner-access` (see `vps/DEPLOY.md`); the node's RPC is reached through an
SSH tunnel, never directly.

## Build

```
cargo build --release                                   # native
cargo build --release --target x86_64-pc-windows-gnu    # Windows (needs mingw-w64)
```

## Use

```
note-miner miner.conf     # rpc_url, rpc_user, rpc_password, address, blocks, threads
note-miner --self-test    # header layout + scrypt against the Litecoin genesis block
note-miner --bench        # kH/s per thread
```

Before hashing, the miner submits its block layout to the node as a
`proposal`, so a construction error shows up immediately instead of after a
found block.

## Test

`mocknode.py [segwit]` is a fake node that hands out an easy template and
fully validates every submitted block (header fields, BIP34 height, coinbase
value and outputs, witness commitment, merkle root, scrypt proof of work):

```
python3 mocknode.py &            # or: python3 mocknode.py segwit
note-miner mock.conf             # rpc_url=http://127.0.0.1:19444
```
