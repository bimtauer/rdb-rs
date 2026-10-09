from pathlib import Path

import pytest

import rdb

DUMPS = Path(__file__).resolve().parent.parent / "dumps"


def test_invoke_rdb():
    expected = [
        {"type": "select_db", "db": 0},
        {
            "type": "set",
            "key": b"intset_16",
            "members": [b"32764", b"32765", b"32766"],
            "expiry": None,
        },
        {"type": "checksum", "checksum": b""},
    ]

    parser = rdb.RdbDecoder(path=str(DUMPS / "intset_16.rdb"))

    assert list(parser) == expected


def test_redis_8_6_dump_accepts_pathlike_and_skips_streams():
    decoder = rdb.RdbDecoder(DUMPS / "redis_8_6_all_types.rdb")
    assert decoder.version == 13

    keys = {item["key"]: item for item in decoder if "key" in item and item["type"] != "aux_field"}

    assert b"stream" not in keys
    assert keys[b"hfe_small"]["values"] == {b"f1": b"v1", b"f2": b"v2", b"f3": b"v3"}
    assert keys[b"lp_ints"]["values"][:2] == [b"-1", b"-5000"]
    assert keys[b"ttl_str"]["expiry"] is not None
    assert keys[b"str"]["expiry"] is None


def test_non_existing_path_raises_file_not_found():
    with pytest.raises(FileNotFoundError):
        rdb.RdbDecoder(path=str(DUMPS / "nonexistent.rdb"))


def test_non_rdb_file_raises_value_error():
    with pytest.raises(ValueError):
        rdb.RdbDecoder(path=str(DUMPS / "json" / "intset_16.json"))
