# miner-widget-stats

Shows mining hashrate and room temperature in the macOS menu bar.

[![CI](https://github.com/koprjaa/miner-widget-stats/actions/workflows/ci.yml/badge.svg)](https://github.com/koprjaa/miner-widget-stats/actions/workflows/ci.yml)
[![MIT license](https://img.shields.io/badge/license-MIT-A31F34?style=flat-square)](LICENSE)

The widget reads your five-minute hashrate from Braiins Pool and the ambient temperature from a Tuya Cloud sensor. It refreshes immediately on startup, then every 300 seconds by default.

## Install

Requires macOS and Python 3.10 or later. Run these commands from the repository directory:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

Create a `.env` file in the repository root. Set `TOKEN` to your Braiins Pool API token. For temperature readings, set `TUYA_ACCESS_ID`, `TUYA_ACCESS_SECRET`, and `TUYA_DEVICE_ID`. The sensor must report `va_temperature` in tenths of a degree Celsius.

`TUYA_REGION` defaults to `eu`; change it if your Tuya account uses another region. `UPDATE_INTERVAL` controls the refresh interval in seconds. Either service can remain unconfigured; its reading will display as `--`.

## Use

Start the menu bar widget:

```bash
python miner_widget.py
```

The menu provides manual refresh, log opening, and quit actions. Logs are written to `logs/miner.log` beside the script.

To inspect the Tuya device status and temperature separately:

```bash
python get_api.py
```

For startup at login, edit `com.miner.widget.plist` before installing it as a LaunchAgent. Its working directory, command, and log paths point to the original author's machine.

## License

[MIT](LICENSE)
