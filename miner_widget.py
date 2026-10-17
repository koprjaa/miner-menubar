#!/usr/bin/env python3
#
# Project: miner-widget-stats
# File:    miner_widget.py
#
# Description:
# macOS menu bar widget that shows the mining pool hash rate and the room temperature.
#
# Author:
# Jan Alexandr Kopřiva
# jan.alexandr.kopriva@gmail.com
#
# License: MIT
#

"""macOS menu bar widget for a mining pool account.

The title carries two readings side by side: the five minute hash rate from
pool.braiins.com and the room temperature from a Tuya sensor. They come from
unrelated services, so either one can fail on its own and the other is still
shown. A refresh runs every UPDATE_INTERVAL seconds on a background thread.
"""

import logging
import os
import socket
import subprocess
import threading
import time
from logging.handlers import RotatingFileHandler
from pathlib import Path

import requests
import rumps
from dotenv import load_dotenv

import get_api
from hashrate import format_hashrate, format_temperature, format_title
from tuya import read_temperature

load_dotenv()

TOKEN = os.getenv("TOKEN")
COIN = os.getenv("COIN", "btc")
BASE_URL = os.getenv("BASE_URL", "https://pool.braiins.com")
UPDATE_INTERVAL = int(os.getenv("UPDATE_INTERVAL", "300"))
MAX_RETRIES = int(os.getenv("MAX_RETRIES", "3"))
RETRY_DELAY = int(os.getenv("RETRY_DELAY", "5"))
HEADERS = {"Pool-Auth-Token": TOKEN} if TOKEN else {}

# LSUIElement hides the dock icon. Apple defines the spelling, so it
# cannot be capitalized.
os.environ["LSUIElement"] = "1"  # noqa: SIM112


class Logger:
    """One logger for the whole application: a rotating file plus the console."""

    _instance = None
    _initialized = False

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def __init__(self):
        """Build the handlers on the first instance only."""
        if not self._initialized:
            self._initialized = True
            self._setup_logger()

    def _setup_logger(self):
        logs_dir = Path(__file__).resolve().parent / "logs"
        logs_dir.mkdir(parents=True, exist_ok=True)

        log_path = logs_dir / "miner.log"
        formatter = logging.Formatter("%(asctime)s - %(levelname)s - %(message)s")

        file_handler = RotatingFileHandler(
            log_path,
            maxBytes=1024 * 1024,  # 1MB
            backupCount=5,
        )
        file_handler.setFormatter(formatter)

        console_handler = logging.StreamHandler()
        console_handler.setFormatter(formatter)

        self.logger = logging.getLogger("miner")
        self.logger.setLevel(logging.INFO)
        self.logger.propagate = False
        self.logger.handlers = []
        self.logger.addHandler(file_handler)
        self.logger.addHandler(console_handler)

    def get_logger(self):
        return self.logger


logger = Logger().get_logger()


def check_internet():
    """True when 8.8.8.8 answers on the DNS port.

    Checked before every refresh so an offline machine shows "Offline" instead of
    waiting out two HTTP timeouts.
    """
    try:
        socket.create_connection(("8.8.8.8", 53), timeout=3)
    except OSError:
        logger.warning("No internet connection")
        return False
    else:
        logger.debug("Internet connection OK")
        return True


def open_log():
    """Open the log file in the default editor."""
    log_path = Path(__file__).resolve().parent / "logs" / "miner.log"
    if log_path.exists():
        logger.info(f"Opening log file: {log_path}")
        subprocess.run(["open", str(log_path)], check=False)
    else:
        logger.error(f"Log file does not exist: {log_path}")


class MinerWidget(rumps.App):
    """Menu bar widget showing the pool hash rate and the room temperature."""

    def __init__(self):
        super().__init__("Miner", quit_button=None, icon=None)
        self.title = "--"
        self.update_thread = None
        self.running = True

        self.menu = [
            rumps.MenuItem("Aktualizovat", callback=self.force_update),
            rumps.MenuItem("Otevřít log", callback=lambda _: open_log()),
            rumps.MenuItem("Ukončit", callback=self.quit),
            None,  # separator
        ]

        logger.info("Widget initialized")

        # Refresh once now, otherwise the title reads "--" until the first interval elapses.
        self.update_thread = threading.Thread(target=self.update_status, daemon=True)
        self.update_thread.start()

    def force_update(self, _):
        """Refresh on demand. rumps passes the menu item, which is not used."""
        logger.info("Manual refresh started")
        self.update_status_once()
        logger.info("Manual refresh finished")

    def update_status_once(self):
        """Read the pool hash rate and the Tuya temperature, then set the title."""
        if not check_internet():
            logger.warning("No network")
            self.title = "Offline"
            return

        logger.info("Fetching data from pool.braiins.com")
        profile, workers = self.fetch_data()

        # Read the temperature even when the pool call failed: the two are independent,
        # and a temperature alone is still worth showing.
        try:
            logger.info("Reading temperature from the Tuya API")
            token = get_api.get_access_token()
            status = get_api.get_device_status(token)
            temp = format_temperature(read_temperature(status))
            logger.info(f"Temperature: {temp}°C")
        except Exception:
            logger.exception("Could not read the temperature")
            temp = "--"

        if profile and workers:
            try:
                data = profile[COIN]
                hr5 = data["hash_rate_5m"]
                unit = data["hash_rate_unit"]
                logger.info(f"Hash rate: {hr5} {unit}")

                display_rate = format_hashrate(hr5, unit)
                logger.debug(f"Displayed hash rate: {display_rate}")

                new_title = format_title(display_rate, temp)
                logger.info(f"Setting title to: {new_title}")
                self.title = new_title

            except Exception:
                logger.exception("Could not update the title")
                self.title = format_title("Error", temp)
        else:
            logger.warning("Could not read data from pool.braiins.com")
            self.title = format_title("--", temp)

    def fetch_data(self):
        """Read the profile and the workers, retrying on failure.

        Returns (None, None) when the token is missing or every attempt fails.
        """
        if not TOKEN:
            logger.warning("Pool token is not set, skipping pool.braiins.com")
            return None, None

        for attempt in range(MAX_RETRIES):
            try:
                profile_url = f"{BASE_URL}/accounts/profile/json/{COIN}/"
                workers_url = f"{BASE_URL}/accounts/workers/json/{COIN}/"
                logger.info(f"Attempt {attempt + 1}/{MAX_RETRIES}")

                session = requests.Session()

                logger.debug(f"Reading profile from: {profile_url}")
                profile = session.get(profile_url, headers=HEADERS, timeout=10)
                profile.raise_for_status()
                profile_data = profile.json()

                logger.debug(f"Reading workers from: {workers_url}")
                workers = session.get(workers_url, headers=HEADERS, timeout=10)
                workers.raise_for_status()
                workers_data = workers.json()

                logger.info("Data read")
                return profile_data, workers_data

            except Exception:
                logger.exception(f"Could not read data (attempt {attempt + 1}/{MAX_RETRIES})")
                if attempt < MAX_RETRIES - 1:
                    logger.info(f"Waiting {RETRY_DELAY}s before the next attempt")
                    time.sleep(RETRY_DELAY)
                continue

        return None, None

    def update_status(self):
        """Refresh loop. Runs on its own thread until quit() clears the running flag."""
        logger.info("Starting the refresh loop")
        while self.running:
            try:
                self.update_status_once()
                logger.debug(f"Sleeping {UPDATE_INTERVAL}s until the next refresh")
                time.sleep(UPDATE_INTERVAL)
            except Exception:
                logger.exception("Error in the refresh thread")
                time.sleep(RETRY_DELAY)

    def run(self):
        logger.info("Starting the application")
        super().run()

    def quit(self):
        """Stop the refresh thread before letting rumps tear the application down."""
        logger.info("Shutting down")
        self.running = False

        if self.update_thread and self.update_thread.is_alive():
            self.update_thread.join(timeout=5)

        super().quit()


if __name__ == "__main__":
    # Hide the Dock icon. Pairs with the LSUIElement environment variable set at import.
    try:
        import AppKit

        info = AppKit.NSBundle.mainBundle().infoDictionary()
        info["LSUIElement"] = True
    except ImportError:
        pass

    app = MinerWidget()
    app.run()
