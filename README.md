# rdb-rs

[![Rust](https://github.com/bimtauer/rdb-rs/actions/workflows/ci.yaml/badge.svg)](https://github.com/bimtauer/rdb-rs/actions/workflows/ci.yaml)
[![Python](https://github.com/bimtauer/rdb-rs/actions/workflows/python-cd.yaml/badge.svg)](https://github.com/bimtauer/rdb-rs/actions/workflows/python-cd.yaml)
[![crates.io](https://img.shields.io/crates/v/rdb.svg)](https://crates.io/crates/rdb)
[![PyPI](https://img.shields.io/pypi/v/rdb-py.svg)](https://pypi.org/project/rdb-py/)

Fast parser for Redis RDB dump files, as a Rust library, a command line tool and a Python package.

Use it to look inside a Redis snapshot without loading it into Redis: inspect or export keys, convert a dump
to JSON, replay it as Redis commands, or feed it into your own analysis code.

## Supported Redis versions

RDB format versions 1 to 13, i.e. dumps written by Redis up to **8.6**. CI parses dumps produced by real
Redis 6.2, 7.0, 7.2, 7.4, 8.0, 8.2, 8.4 and 8.6 servers.

| Data | Support |
|---|---|
| Strings, lists, sets, sorted sets, hashes (all encodings) | Decoded |
| Key expiry | Decoded (absolute Unix time in ms) |
| Hashes with field TTLs (Redis 7.4+) | Fields and values decoded, per-field TTLs not reported |
| Streams | Skipped: the file parses, stream keys are omitted |
| Module values (RedisJSON, RediSearch, ...) | Skipped |
| Lua functions, LRU/LFU info, cluster slot info, key metadata | Skipped |

Files from newer Redis versions are rejected with an "unsupported RDB version" error rather than misread.

## Command line

```sh
cargo install rdb
```

```text
Usage: rdb [options] dump.rdb

Options:
  -f, --format <FORMAT>  Format to output. Valid: json, plain, nil, protocol
  -k, --keys <KEYS>      Keys to show. Can be a regular expression
  -d, --databases <DB>   Database to show. Can be specified multiple times
  -t, --type <TYPE>      Type to show. Can be specified multiple times
  -o, --output <FILE>    Output file path. If not specified, writes to stdout
```

Types are `string`, `list`, `set`, `sortedset` and `hash`.

```console
$ rdb --format json --keys '^(str|zset|hfe_small)$' dump.rdb
[{"str":"hello","zset":{"a":"1","b":"2"},"hfe_small":{"f1":"v1","f2":"v2","f3":"v3"}}]

$ rdb --format plain --keys '^zset$' dump.rdb | grep zset
db=0 zset[0] -> {a, score=1}
db=0 zset[1] -> {b, score=2}

$ rdb --format protocol dump.rdb | redis-cli --pipe   # replay into another server
```

`json` prints one object per database, `plain` one line per value, `protocol` emits
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/) commands (`SET`, `RPUSH`, `HSET`, `SADD`,
`ZADD`, `PEXPIREAT`) and `nil` only validates the file. Errors go to stderr with exit code 1.

## Rust

```toml
[dependencies]
rdb = "0.4"
```

```rust
use std::fs::File;
use std::io::BufReader;
use rdb::{RdbDecoder, RdbValue, filter::Simple};

fn main() -> Result<(), rdb::RdbError> {
    let file = BufReader::new(File::open("dump.rdb")?);
    for item in RdbDecoder::new(file, Simple::new())? {
        if let RdbValue::String { key, value, expiry } = item? {
            println!("{:?} = {:?} (expires {:?})", key, value, expiry);
        }
    }
    Ok(())
}
```

`rdb::parse(reader, formatter, filter)` renders a dump through one of the built-in formatters or your own
`Formatter` implementation. API documentation: <https://docs.rs/rdb>.

## Python

```sh
pip install rdb-py
```

Wheels are published for Linux, macOS and Windows, for CPython 3.10+ (including free-threaded 3.14t).

```python
import rdb

decoder = rdb.RdbDecoder("dump.rdb")  # str or pathlib.Path
print(decoder.version)                # RDB format version, e.g. 13

for item in decoder:
    if item["type"] == "hash":
        print(item["key"], item["values"], item["expiry"])
```

Each item is a `dict` with a `type` field. Keys and values are `bytes`; `expiry` is a Unix timestamp in
milliseconds or `None`.

| `type` | Other fields |
|---|---|
| `string` | `key`, `value`, `expiry` |
| `list` | `key`, `values: list[bytes]`, `expiry` |
| `set` | `key`, `members: list[bytes]`, `expiry` |
| `sorted_set` | `key`, `values: list[tuple[float, bytes]]` (score, member), `expiry` |
| `hash` | `key`, `values: dict[bytes, bytes]`, `expiry` |
| `select_db` | `db` |
| `resize_db` | `db_size`, `expires_size` |
| `aux_field` | `key`, `value` (metadata such as `redis-ver`) |
| `checksum` | `checksum` (always the last item) |

The package ships type hints (`TypedDict` per item type). Opening a missing file raises `FileNotFoundError`;
a file that is not a supported RDB file, or malformed data during iteration, raises `ValueError`.

## Development

```sh
cargo test                                   # unit, fixture, CLI and Redis container tests (needs Docker)
cargo test -- --skip test_redis_protocol     # without Docker
uv sync && uv run pytest                     # Python bindings
```

`tests/dumps/*.rdb` are compared byte-for-byte against the expected outputs in `tests/dumps/{json,plain,protocol}/`.
To add a fixture, put the dump there and generate the three outputs with
`cargo run -- --format <format> -o tests/dumps/<format>/<name>.<format> tests/dumps/<name>.rdb`, then check them by hand.

The file format is described in [docs/RDB_File_Format.md](docs/RDB_File_Format.md) and its evolution in
[docs/RDB_Version_History.md](docs/RDB_Version_History.md). Changes are listed in the [changelog](CHANGELOG.md).

## History

rdb-rs was written by [Jan-Erik Rediger](https://github.com/badboy) ([badboy/rdb-rs](https://github.com/badboy/rdb-rs),
now archived), inspired by [redis-rdb-tools](https://github.com/sripathikrishnan/redis-rdb-tools). Since 2024 it is
maintained here by [Tim Bauer](https://github.com/bimtauer). Bugs and ideas: [issues](https://github.com/bimtauer/rdb-rs/issues).

## License

MIT, see [LICENSE](LICENSE).
