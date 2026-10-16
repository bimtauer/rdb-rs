# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
 - `JSON`, `Plain` and `Protocol::from_writer` to render into any `io::Write`
 - Provided method `Formatter::flush`, called by `parse` after `end_rdb`
 - Python: type stub checked against the extension (stubtest); `RdbDecoder` is marked `@final`

### Fixed
 - Malformed files could crash or exhaust memory: an LZF string claiming a huge length allocated it
   up front, and a stream with a huge consumer PEL count overflowed. Both found by fuzzing, now errors
 - The CLI panicked when its output pipe closed early (`rdb --format plain dump.rdb | head`); it now
   stops quietly
 - `-o` with an unwritable path silently wrote to stdout; it now fails with an error
 - Formatters panicked on write errors; `parse` now returns them
 - JSON output contained `{}` for databases whose keys were all filtered out, and was invalid for keys
   before the first database selector
 - `Nil::new(Some(path))` created or truncated the file
 - `Formatter::resizedb` was never called
 - Output files were written unbuffered

### Changed
 - Python 3.11 or newer is required (3.10 reached end of life)
 - `formatter::write_str` is deprecated

## [0.4.0] - 2026-10-14

### Added
 - RDB versions 10-13: Redis 7.0 up to 8.6 ([#8](https://github.com/bimtauer/rdb-rs/issues/8))
 - Hashes with field TTLs (Redis 7.4+) are decoded; the per-field TTLs are not reported
 - Streams, module values (e.g. RedisJSON), Lua functions, cluster slot info and key metadata are
   skipped, so dumps containing them parse
 - `RdbDecoder::new` is public for iterating over items, with `RdbDecoder::version()`
 - `RdbValue` is exported from the crate root
 - Error variants `InvalidHeader`, `UnsupportedVersion` and `Unsupported`
 - Python: type hints, `RdbDecoder.version`, `pathlib.Path` arguments, abi3 wheels for CPython 3.10+
   and free-threaded 3.14t wheels
 - Redis container tests for 8.0, 8.2, 8.4 and 8.6, writing streams, functions and field TTLs
 - Fixtures from Redis 7.4, Redis 8.6 and redis-stack dumps

### Fixed
 - Any Redis 7+ dump containing a Lua function failed with `Unknown encoding type: 245`
 - Streams panicked (`todo!()`)
 - Negative 13-bit and all 24-bit listpack integers were decoded wrongly (e.g. `-1` as `65535`,
   `100000` as `390`)
 - A key's expiry was applied to every following key without one
 - Key filters (`--type`, `--keys`, `--databases`) failed on several encodings and emitted spurious
   database switches for skipped keys
 - 64-bit lengths (`0x81`) were read as 32-bit
 - Seconds-precision expiry (`EXPIRETIME`, RDB < 3) was read big-endian
 - Truncated files were reported as complete instead of failing
 - Invalid UTF-8 values containing `"` or `\` produced invalid JSON
 - Corrupt input panicked or could trigger huge allocations; it now returns an error
 - Python: files were read unbuffered (about 10x slower); any open failure raised `FileNotFoundError`
 - Python: `rdb-py` declared `cffi` and `maturin` as runtime dependencies

### Changed
 - `RdbParserBuilder::build` returns `RdbResult` instead of panicking
 - The CLI prints errors to stderr and exits with code 1 instead of panicking
 - Iteration stops after the first error
 - Rust edition 2024, minimum Rust version 1.87; pyo3 0.29

### Removed
 - Unmaintained `rustc-serialize` dependency
 - Unused `types::EncodingType`
 - `utils/PKGBUILD` (pointed at the archived repository and a removed Makefile)

## [0.3.0] - 2025-03-29

### Added
 - New RDB version and Datatypes
 - Rust based integration tests
 - Fixtures for protocol and plain output
 - Redis integration test covering 6.2 - 7.4
 - Option to output to file
 - Error handling with thiserror
 - Support for new encoding types:
    - listpack
    - quicklist
    - sorted set v2
 - Python bindings with Maturin

### Changed
 - Ported CLI to clap
 - Encoding of non-ascii characters - previously escaped, resulting in possible duplicate json keys, now as hex string
 - Separated decoding and formatting logic

### Removed
 - Previous docs and build pipeline


---
# Previous:

### 0.2.1 - 2016-08-03

* Bug fix: Correctly handle skipping blobs
* Fix: Pin dependency versions

### 0.2.0 - 2015-04-04

* Make it work on Rust 1.0 beta

### 0.1.0 - 2015-03-23

Initial release with basic functionality
