# Zallet NU7 Testnet Quick Start

This directory contains the ready-to-use Zallet config for the Valar Group NU7 PoW testnet.

## Downloaded release

Download `zallet-<tag>-ubuntu-x86_64.tar.gz` from the GitHub release, then run:

```sh
tar -xzf zallet-*-ubuntu-x86_64.tar.gz
cd zallet-*-ubuntu-x86_64

export ZALLET_DATADIR="$HOME/.zallet-nu7-testnet"
export ZALLET_CONFIG="$(pwd)/zallet.toml"

./zallet --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" init-wallet-encryption
./zallet --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" generate-mnemonic
./zallet --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" start
```

Zallet connects to Zebra RPC at `155.138.237.238:18232` and exposes local JSON-RPC at `127.0.0.1:28232`.
The local Zallet JSON-RPC username is `zallet` and the password is `nu7-testnet`.

Example wallet RPC call:

```sh
curl --user zallet:nu7-testnet \
  --data-binary '{"jsonrpc":"2.0","id":"walletinfo","method":"getwalletinfo","params":[]}' \
  -H 'content-type: application/json' \
  http://127.0.0.1:28232/
```

## Build from source

Build Zallet for this network:

```sh
export CXXFLAGS='-include cstdint'
export RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235" --cfg zcash_unstable="nsm"'
cargo build --release --locked -p zallet
```

Use a dedicated data directory so this testnet wallet cannot collide with mainnet, testnet, or regtest state:

```sh
ZALLET=target/release/zallet
ZALLET_DATADIR="$HOME/.zallet-nu7-testnet"
ZALLET_CONFIG="$(pwd)/contrib/nu7-testnet/zallet.toml"

"$ZALLET" --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" init-wallet-encryption
"$ZALLET" --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" generate-mnemonic
"$ZALLET" --datadir "$ZALLET_DATADIR" --config "$ZALLET_CONFIG" start
```
