"""Fast Redis RDB dump file parser, powered by Rust.

Iterate over an RDB file with :class:`RdbDecoder`::

    import rdb

    for item in rdb.RdbDecoder("dump.rdb"):
        if item["type"] == "string":
            print(item["key"], item["value"], item["expiry"])

Keys, values and members are ``bytes`` exactly as stored by Redis.
``expiry`` is an absolute Unix timestamp in milliseconds, or ``None``.
Streams and module values (e.g. RedisJSON) are skipped; per-field hash
TTLs (Redis 7.4+) are not reported.
"""

from os import PathLike
from typing import Literal, TypedDict, Union

class SelectDb(TypedDict):
    """Following keys belong to database ``db``."""

    type: Literal["select_db"]
    db: int

class ResizeDb(TypedDict):
    """Size hints for the current database."""

    type: Literal["resize_db"]
    db_size: int
    expires_size: int

class AuxField(TypedDict):
    """File metadata such as ``redis-ver`` or ``ctime``."""

    type: Literal["aux_field"]
    key: bytes
    value: bytes

class Checksum(TypedDict):
    """Trailing CRC64 (8 bytes, empty before RDB 5). Always the last item."""

    type: Literal["checksum"]
    checksum: bytes

class String(TypedDict):
    type: Literal["string"]
    key: bytes
    value: bytes
    expiry: int | None

class Hash(TypedDict):
    type: Literal["hash"]
    key: bytes
    values: dict[bytes, bytes]
    expiry: int | None

class List(TypedDict):
    type: Literal["list"]
    key: bytes
    values: list[bytes]
    expiry: int | None

class Set(TypedDict):
    type: Literal["set"]
    key: bytes
    members: list[bytes]
    expiry: int | None

class SortedSet(TypedDict):
    type: Literal["sorted_set"]
    key: bytes
    values: list[tuple[float, bytes]]
    """``(score, member)`` pairs."""
    expiry: int | None

Item = Union[SelectDb, ResizeDb, AuxField, Checksum, String, Hash, List, Set, SortedSet]

class RdbDecoder:
    """Iterator over the items of an RDB file, in file order.

    Raises ``OSError`` (e.g. ``FileNotFoundError``) if the file cannot be
    opened and ``ValueError`` if it is not a supported RDB file. Iteration
    raises ``ValueError`` on malformed data.
    """

    def __init__(self, path: str | PathLike[str]) -> None: ...
    @property
    def version(self) -> int:
        """RDB format version declared in the file header (1-13)."""
    def __iter__(self) -> RdbDecoder: ...
    def __next__(self) -> Item: ...
