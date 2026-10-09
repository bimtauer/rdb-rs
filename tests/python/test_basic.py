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


def test_non_existing_path_raises_file_not_found():
    with pytest.raises(FileNotFoundError):
        rdb.RdbDecoder(path=str(DUMPS / "nonexistent.rdb"))


def test_non_rdb_file_raises_value_error():
    with pytest.raises(ValueError):
        rdb.RdbDecoder(path=str(DUMPS / "json" / "intset_16.json"))
