# miner-menubar

Shows mining hashrate and room temperature in the macOS menu bar.

[![CI](https://github.com/koprjaa/miner-menubar/actions/workflows/ci.yml/badge.svg)](https://github.com/koprjaa/miner-menubar/actions/workflows/ci.yml)
[![MIT license](https://img.shields.io/badge/license-MIT-A31F34?style=flat-square)](LICENSE)

The widget reads your five-minute hashrate from Braiins Pool and the ambient temperature from a Tuya Cloud sensor. It refreshes immediately on startup, then every 300 seconds by default. The title looks like `12.5TH/s 23.5°C`; a source that fails shows `--`, and a machine with no network shows `Offline`.

## Install

Install a current stable Rust toolchain, then build from the repository directory:

```bash
cargo build --release
```

The executable is `target/release/miner-menubar`. It runs on macOS only.

## Configure

Create a `.env` file in the repository root. The widget looks for it next to the executable and in the folders above it, then in the working directory.

| Variable | Default | Meaning |
| --- | --- | --- |
| `TOKEN` | none | Braiins Pool API token. Without it the pool part shows `--`. |
| `COIN` | `btc` | Coin in the pool API. |
| `BASE_URL` | `https://pool.braiins.com` | Pool API address. |
| `UPDATE_INTERVAL` | `300` | Seconds between refreshes. At least 1. |
| `MAX_RETRIES` | `3` | Pool attempts per refresh. |
| `RETRY_DELAY` | `5` | Seconds between pool attempts. |
| `TUYA_ACCESS_ID`, `TUYA_ACCESS_SECRET`, `TUYA_DEVICE_ID` | none | Tuya Cloud project and device. Without them the temperature shows `--`. |
| `TUYA_REGION` | `eu` | Tuya data center, for example `us` or `cn`. |

The sensor must report `va_temperature` in tenths of a degree Celsius.

## Use

```bash
./target/release/miner-menubar
```

The menu has manual refresh, log opening, and quit. Logs go to `logs/miner.log` beside the `.env` file.

For startup at login, edit `com.miner.widget.plist` before installing it as a LaunchAgent. Its binary, working directory, and log paths point to the original author's machine.

## License

[MIT](LICENSE)
