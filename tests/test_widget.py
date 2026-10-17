#
# Project: miner-widget-stats
# File:    test_widget.py
#
# Description:
# Tests for the Tuya request signing and the value formatting. No account, no pool, no menu bar.
#
# Author:
# Jan Alexandr Kopřiva
# jan.alexandr.kopriva@gmail.com
#
# License: MIT
#

"""Tests for the widget logic. No Tuya account, no pool, no menu bar.

get_api.py imports requests and dotenv and reads the environment, so it is left
alone here. tuya.py and hashrate.py hold everything worth checking.
"""

import hashlib
import hmac

import pytest

from hashrate import TO_TERAHASH, format_hashrate, format_temperature, format_title
from tuya import (
    TEMPERATURE_SCALE,
    TuyaError,
    base_url,
    make_headers,
    read_temperature,
    signature,
    unwrap,
)

ACCESS_ID = "test-id"
SECRET = "test-secret"
PATH = "/v1.0/token?grant_type=1"


# --- signature --------------------------------------------------------------


def test_the_signature_matches_the_documented_string_to_sign():
    """Tuya signs GET, a hash of the empty body, an empty header line and the path."""
    ts, nonce = "1700000000000", "abc123"
    empty_body = hashlib.sha256(b"").hexdigest()
    expected_base = ACCESS_ID + ts + nonce + f"GET\n{empty_body}\n\n{PATH}"
    expected = hmac.new(SECRET.encode(), expected_base.encode(), hashlib.sha256).hexdigest().upper()
    assert signature(ACCESS_ID, SECRET, PATH, ts, nonce) == expected


def test_a_token_goes_into_the_signature():
    ts, nonce = "1700000000000", "abc123"
    without = signature(ACCESS_ID, SECRET, PATH, ts, nonce)
    with_token = signature(ACCESS_ID, SECRET, PATH, ts, nonce, "tok")
    assert without != with_token


def test_the_signature_is_uppercase_hex():
    sig = signature(ACCESS_ID, SECRET, PATH, "1", "n")
    assert sig == sig.upper()
    assert len(sig) == 64


@pytest.mark.parametrize(
    "changed",
    [
        {"path": "/v1.0/devices/x/status"},
        {"ts": "1700000000001"},
        {"nonce": "different"},
    ],
)
def test_every_signed_part_changes_the_signature(changed):
    """A part that does not affect the result is a part the server will reject."""
    args = {"path": PATH, "ts": "1700000000000", "nonce": "abc123"}
    baseline = signature(ACCESS_ID, SECRET, **args)
    assert signature(ACCESS_ID, SECRET, **{**args, **changed}) != baseline


def test_a_different_secret_gives_a_different_signature():
    ts, nonce = "1", "n"
    assert signature(ACCESS_ID, SECRET, PATH, ts, nonce) != signature(ACCESS_ID, "other", PATH, ts, nonce)


# --- make_headers -----------------------------------------------------------


def test_the_headers_carry_everything_the_api_needs():
    headers = make_headers(ACCESS_ID, SECRET, PATH)
    assert headers["client_id"] == ACCESS_ID
    assert headers["sign_method"] == "HMAC-SHA256"
    assert headers["t"].isdigit()
    assert len(headers["nonce"]) == 32
    assert len(headers["sign"]) == 64


def test_the_access_token_header_appears_only_with_a_token():
    assert "access_token" not in make_headers(ACCESS_ID, SECRET, PATH)
    assert make_headers(ACCESS_ID, SECRET, PATH, "tok")["access_token"] == "tok"


def test_two_requests_get_different_nonces():
    """A repeated nonce lets a signed request be replayed."""
    first = make_headers(ACCESS_ID, SECRET, PATH)
    second = make_headers(ACCESS_ID, SECRET, PATH)
    assert first["nonce"] != second["nonce"]


def test_the_headers_are_signed_with_their_own_timestamp_and_nonce():
    headers = make_headers(ACCESS_ID, SECRET, PATH, "tok")
    assert headers["sign"] == signature(ACCESS_ID, SECRET, PATH, headers["t"], headers["nonce"], "tok")


# --- base_url ---------------------------------------------------------------


@pytest.mark.parametrize(
    ("region", "expected"),
    [
        ("eu", "https://openapi.tuyaeu.com"),
        ("us", "https://openapi.tuyaus.com"),
        ("cn", "https://openapi.tuyacn.com"),
    ],
)
def test_the_region_goes_into_the_host(region, expected):
    assert base_url(region) == expected


def test_the_default_region_is_europe():
    assert base_url() == base_url("eu")


# --- read_temperature -------------------------------------------------------


def test_the_reading_is_scaled_from_tenths_of_a_degree():
    status = [{"code": "va_temperature", "value": 235}]
    assert read_temperature(status) == 23.5


def test_the_temperature_is_picked_out_of_the_other_readings():
    status = [
        {"code": "va_humidity", "value": 55},
        {"code": "battery_percentage", "value": 90},
        {"code": "va_temperature", "value": 212},
    ]
    assert read_temperature(status) == 21.2


@pytest.mark.parametrize(
    "status",
    [
        [],
        None,
        [{"code": "va_humidity", "value": 55}],
        [{"code": "va_temperature"}],
        [{"code": "va_temperature", "value": None}],
        [{"code": "va_temperature", "value": "warm"}],
    ],
)
def test_a_missing_or_unusable_reading_gives_none(status):
    assert read_temperature(status) is None


def test_a_reading_below_zero_survives():
    assert read_temperature([{"code": "va_temperature", "value": -55}]) == -5.5


def test_the_scale_is_a_named_constant():
    assert TEMPERATURE_SCALE == 10.0


# --- unwrap -----------------------------------------------------------------


def test_a_successful_response_gives_its_result():
    assert unwrap({"success": True, "result": {"a": 1}}, "Token") == {"a": 1}


def test_a_refused_response_raises_with_the_payload():
    with pytest.raises(TuyaError, match="Token failed"):
        unwrap({"success": False, "msg": "sign invalid"}, "Token")


def test_a_response_without_a_success_field_is_treated_as_refused():
    with pytest.raises(TuyaError):
        unwrap({"result": {}}, "Status")


# --- format_hashrate --------------------------------------------------------


@pytest.mark.parametrize(
    ("value", "unit", "expected"),
    [
        (12500, "Gh/s", "12.5TH/s"),  # converted once it reaches a terahash
        (1000, "Gh/s", "1.0TH/s"),  # exactly at the boundary
        (999, "Gh/s", "999.0Gh/s"),  # just below, keeps its own unit
        (500, "Gh/s", "500.0Gh/s"),
        (12.5, "TH/s", "12.5TH/s"),
        (1.5, "Ph/s", "1500.0TH/s"),
        (0.5, "Ph/s", "500.0TH/s"),
        (100, "Mh/s", "100.0Mh/s"),
    ],
)
def test_the_readings_the_pool_sends(value, unit, expected):
    assert format_hashrate(value, unit) == expected


def test_an_unknown_unit_is_shown_as_it_arrived():
    """Guessing at a unit the pool invented would print a wrong number."""
    assert format_hashrate(42, "Zh/s") == "42.0Zh/s"


@pytest.mark.parametrize("value", [None, "", "abc", [], {}])
def test_a_reading_that_is_not_a_number_shows_as_unknown(value):
    assert format_hashrate(value, "TH/s") == "--"


def test_a_zero_reading_is_shown_rather_than_hidden():
    """A miner that stopped reads zero, which is exactly what to display."""
    assert format_hashrate(0, "TH/s") == "0.0TH/s"


def test_a_numeric_string_is_accepted():
    assert format_hashrate("12.5", "TH/s") == "12.5TH/s"


@pytest.mark.parametrize("unit", sorted(TO_TERAHASH))
def test_every_known_unit_converts_once_it_is_large_enough(unit):
    """A reading worth ten terahash is shown in TH/s, whatever unit it arrived in.

    Ten rather than one, because 1 / 1e-9 lands a fraction below the boundary
    in binary floating point and would sit on the wrong side of the comparison.
    """
    ten_terahash = 10 / TO_TERAHASH[unit]
    assert format_hashrate(ten_terahash, unit).endswith("TH/s")


@pytest.mark.parametrize("unit", sorted(TO_TERAHASH))
def test_a_reading_below_a_terahash_keeps_its_own_unit(unit):
    """0.1 TH/s reads better as 100 Gh/s than as 0.1 TH/s."""
    below = 0.5 / TO_TERAHASH[unit]
    assert format_hashrate(below, unit).endswith(unit)


# --- format_temperature and format_title ------------------------------------


def test_the_temperature_is_shown_to_one_decimal():
    assert format_temperature(23.456) == "23.5"


def test_a_missing_temperature_shows_as_unknown():
    assert format_temperature(None) == "--"


def test_a_temperature_below_zero_keeps_its_sign():
    assert format_temperature(-5.5) == "-5.5"


def test_the_title_puts_both_readings_together():
    assert format_title("12.5TH/s", "23.5") == "12.5TH/s 23.5°C"


def test_the_title_still_reads_when_a_part_is_missing():
    assert format_title("--", "--") == "-- --°C"
