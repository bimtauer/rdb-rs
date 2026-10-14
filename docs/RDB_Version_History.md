% RDB Version History
## RDB Version History

This document tracks the changes made to the dump file format over time.

An RDB file is forwards compatible. An older dump file format will always work with a newer version of Redis.

Versions 8 to 13 below list the type and opcode constants added to `src/rdb.h` in each Redis release
(compare e.g. [7.4.11](https://github.com/redis/redis/blob/7.4.11/src/rdb.h) and
[8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.h)). Byte layouts are in `rdbLoadObject` in `src/rdb.c`.

| RDB version | Written by Redis |
|---|---|
| 7 | 3.2 |
| 8 | 4.0 |
| 9 | 5.0, 6.0, 6.2 |
| 10 | 7.0 |
| 11 | 7.2 |
| 12 | 7.4, 8.0, 8.2, 8.4 |
| 13 | 8.6 |

## Version 13

Redis 8.6.

* New opcode `KEY_META` (243). Optional per-key prefix written after any expiry/`IDLE`/`FREQ` prefix and directly
  before the value type byte. Followed by a length-encoded class count; each class is a 4-byte class spec followed
  by a module value opcode stream (see `MODULE_2`) terminated by `EOF` (0).
* New type `STREAM_LISTPACKS_4` (26). Every stream saved by 8.6 uses it. Same as type 21, followed by the IDMP
  (idempotent producer) section: duration, max entries, a list of producers with their idempotency ids, and the
  `iids_added` / `iids_duplicates` counters.

## Version 12

Redis 7.4 (also written by 8.0 - 8.4).

* Hash field expiration. Hashes with at least one field TTL use new types:
    * `HASH_METADATA` (24): 8-byte little-endian minimum expiry (ms), then `len` triplets of
      `<ttl:length> <field:string> <value:string>`. A TTL of 0 means none, otherwise it is relative:
      `expire_at = ttl + min_expire - 1`.
    * `HASH_LISTPACK_EX` (25): 8-byte minimum expiry, then a listpack of `field, value, ttl` triplets, where `ttl`
      is an absolute Unix time in ms, or 0 for none.
    * `HASH_METADATA_PRE_GA` (22) and `HASH_LISTPACK_EX_PRE_GA` (23) are the 7.4 release candidate variants
      without the minimum expiry prefix (and with absolute TTLs for type 22).
* New opcode `SLOT_INFO` (244), cluster mode only: three lengths (slot id, slot size, expires slot size).

## Version 11

Redis 7.2.

* New type `SET_LISTPACK` (20): small sets stored as a listpack.
* New type `STREAM_LISTPACKS_3` (21): consumers additionally store an 8-byte active time.
* `FUNCTION` (246) renamed to `FUNCTION_PRE_GA` and `MODULE` (6) to `MODULE_PRE_GA`. Both are release-candidate
  formats Redis itself refuses to load.

## Version 10

Redis 7.0. Ziplists are replaced by listpacks.

* New types `HASH_LISTPACK` (16), `ZSET_LISTPACK` (17) and `LIST_QUICKLIST_2` (18, quicklist nodes are either a
  plain string or a listpack).
* New type `STREAM_LISTPACKS_2` (19): adds first id, max deleted entry id and entries added to the stream, and
  entries read to each consumer group.
* New opcode `FUNCTION2` (245): one string holding the code of a function library.

## Version 9

Redis 5.0 (also written by 6.0 and 6.2).

* New type `STREAM_LISTPACKS` (15).
* New opcodes `MODULE_AUX` (247, module auxiliary data), `IDLE` (248, LRU idle time in seconds) and
  `FREQ` (249, one byte LFU counter). `IDLE` and `FREQ` are key prefixes like the expiry opcodes.

## Version 8

Redis 4.0.

* 64-bit lengths: length byte `0x81` followed by an 8-byte big-endian integer (`0x80` is the 32-bit form).
* New type `ZSET_2` (5): sorted set scores as 8-byte little-endian binary doubles instead of strings.
* Module types `MODULE` (6) and `MODULE_2` (7). A `MODULE_2` value is a 64-bit module id followed by a stream of
  `(opcode, payload)` pairs (`SINT`/`UINT`: length-encoded, `FLOAT`: 4 bytes, `DOUBLE`: 8 bytes, `STRING`: string)
  terminated by opcode `EOF` (0), so it can be skipped without the module.

## Version 7

Introduced 2014-01-08, integrated into Redis 2.9.x.

* New opcode: `RESIZEDB` (251). This encodes hash tables sizes to allow for faster loading.  
  Followed by two length-encoded integers indicating:
    * Database hash table size
    * Expiry hash table size
* New opcode: `AUX` (250). This allows for arbitrary key-value settings. Unknown keys are ignored.  
  Followed by two length-prefixed strings representing the key and value of the setting. Currently implemented fields:
    * `redis-ver`: The Redis Version that wrote the RDB
    * `redis-bits`: Bit architecture of the system that wrote the RDB, either 32 or 64
    * `ctime`: Creation time of the RDB
    * `used-mem`: Used memory of the instance that wrote the RDB
* New encoding type `LIST_QUICKLIST` (14)

Relevant links:

* opcode `AUX`: [redis#206cd219](https://github.com/antirez/redis/commit/206cd219b63c2255c0238cb9c602b65f05e98120), [redis#4c0e8923](https://github.com/antirez/redis/commit/4c0e8923a6cb376c7b2a53fa76ae95f74610285c)
* opcode `RESIZEDB`: [redis#e8614a1a](https://github.com/antirez/redis/commit/e8614a1a77d2989f7be3cb7b24cd88b01f14f17e)
* new type `LIST_QUICKLIST`: [redis#101b3a6e](https://github.com/antirez/redis/commit/101b3a6e42e84e5cb423ef413225d8b8d8cc0bbc), [Redis Quicklist: Do you even list?](https://matt.sh/redis-quicklist-visions)

**Caution**: This breaks backwards-compatibility. Redis 2.8 cannot load a RDB version 7 file.

## Version 6

In previous versions, ziplists used a variable length encoding scheme for integers.
Integers were stored in 16, 32 or 64 bits. In this version, this variable length
encoding system has been extended.

The following additions have been made :

* Integers 0 through 12, both inclusive, are now encoded as part of the entry header
* Numbers between -128 and 127, both inclusive, are stored in 1 byte
* Numbers between -2^23 and 2^23 -1, both inclusive, are stored in 3 bytes

Issue ID: [redis#469](https://github.com/antirez/redis/issues/469)

To migrate to version 5:

* In redis.conf, set `list-max-ziplist-entries` to 0
* Restart Redis Server, and issue the `SAVE` command
* Edit the dump.rdb file, and change the rdb version in the header to `REDIS0005`


## Version 5

This version introduced an 8 byte checksum (CRC64) at the end of the file. If checksum is disabled in redis.conf,
the last 8 bytes will be zeroes.

Issue ID: [redis#366](https://github.com/antirez/redis/issues/366)

To migrate to version 4:

* Delete the last 8 bytes of the file (i.e. after the byte `0xFF`)
* Change the rdb version in the header to `REDIS0004`


## Version 4

This version introduced a new encoding for hashmaps - "Hashmaps encoded as Zip Lists". This version also deprecates
the Zipmap encoding that was used in previous versions.

"Hashmaps encoded as ziplists" has encoding type = 13. The value is parsed like a ziplist, and adjacent entries
in the list are considered key=value pairs in the hashmap.

Issue ID: [redis#285](https://github.com/antirez/redis/pull/285)

To migrate to version 3:

* In redis.conf, set `hash-max-ziplist-entries` to 0
* Restart Redis Server, and issue the `SAVE` command
* Edit the dump.rdb file, and change the rdb version in the header to `REDIS0003`

## Version 3

This version introduced key expiry with millisecond precision.

Earlier versions stored key expiry in the format `0xFD <4 byte timestamp>`. In version 3, key expiry is stored as
`0xFC <8 byte timestamp>`. Here, 0xFD and 0xFC are the opcodes to indicate key expiry in seconds and milliseconds respectively.

Issue ID: [redis#169](https://github.com/antirez/redis/issues/169)

To migrate to version 2:

* If you don't use key expiry, simply change the version in the header to `REDIS0002`
* If you use key expiry, you can still migrate, but there will be some loss in expiry precision. Also, the migration is a bit involved.
* For each key=value pair in the dump file, you will have to convert `0xFC <8 byte timestamp>` to `0xFD <4 byte timestamp>`.
* After converting the timestamps, change the version in the header to `REDIS0002`

## Version 2

This version introduced special encoding for small hashmaps, lists and sets.

Specifically, it introduced the following encoding types:

    REDIS_RDB_TYPE_HASH_ZIPMAP = 9
    REDIS_RDB_TYPE_LIST_ZIPLIST = 10
    REDIS_RDB_TYPE_SET_INTSET = 11
    REDIS_RDB_TYPE_ZSET_ZIPLIST = 12

Commit: [redis#6b52ad87](https://github.com/antirez/redis/commit/6b52ad87c05ca2162a2d21f1f5b5329bf52a7678)

To migrate to version 1:

* In redis.conf, set the following properties to 0 `hash-max-zipmap-entries, list-max-ziplist-entries, set-max-intset-entries, zset-max-ziplist-entries`
* Restart Redis, and issue the SAVE command
* Edit the dump.rdb file, and change the rdb version in the header to `REDIS0001`

