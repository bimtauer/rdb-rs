% RDB File Format
## Redis RDB File Format

> This document covers RDB versions 1 to 13, i.e. every dump format written by Redis up to and including Redis 8.6.
> See [RDB_Version_History.md](RDB_Version_History.md) for which version introduced which change.
> Notes marked *rdb-rs* describe how this crate handles a construct; everything else describes the format itself.

Redis' RDB file is a binary representation of the in-memory store. This binary file is sufficient to completely restore Redis' state.

The RDB file format is optimized for fast read and writes. Where possible LZF compression is used to reduce the file size. In general, objects are prefixed with their lengths, so before reading the object you know exactly how much memory to allocate.

Optimizing for fast read/writes means the on-disk format should be as close as possible to the in-memory representation. This is the approach taken by the RDB file. As a consequence, you cannot parse the RDB file without some understanding of Redis' in-memory representation of data structures.

## High Level Algorithm to parse RDB

At a high level, the RDB file has the following structure

```
----------------------------#
52 45 44 49 53              # Magic String "REDIS"
30 30 31 33                 # RDB Version Number as ASCII string. "0013" = 13
----------------------------
FA                          # Auxiliary field
$string-encoded-key         # May contain arbitrary metadata
$string-encoded-value       # such as Redis version, creation time, used memory, ...
----------------------------
F7 $module-aux              # Module auxiliary data (before the keyspace), optional
F5 $string                  # Function library code, optional, one per library
----------------------------
FE 00                       # Indicates database selector. db number = 00
FB                          # Indicates a resizedb field
$length-encoded-int         # Size of the corresponding hash table
$length-encoded-int         # Size of the corresponding expire hash table
----------------------------# Key-Value pair starts
F4 $len $len $len           # Slot info (cluster mode only), before the first key of each slot
FC $unsigned long           # "expiry time in ms", followed by 8 byte little endian unsigned long
F8 $length-encoded-int      # LRU idle time in seconds, optional
F9 $byte                    # LFU frequency counter, optional
F3 $key-metadata            # Module key metadata, optional (RDB 13)
$value-type                 # 1 byte flag indicating the type of value
$string-encoded-key         # The key, encoded as a redis string
$encoded-value              # The value, encoding depends on $value-type
----------------------------
FD $unsigned int            # "expiry time in seconds", followed by 4 byte little endian int (old files)
$value-type                 # 1 byte flag indicating the type of value
$string-encoded-key         # The key, encoded as a redis string
$encoded-value              # The value, encoding depends on $value-type
----------------------------
$value-type                 # key-value pair without expiry
$string-encoded-key
$encoded-value
----------------------------
FE $length-encoding         # Previous db ends, next db starts.
----------------------------
...                         # Additional key-value pairs, databases, ...

F7 $module-aux              # Module auxiliary data (after the keyspace), optional
FF                          ## End of RDB file indicator
8-byte-checksum             ## CRC64 checksum of the entire file.
```

Redis 8.6 writes the parts in this order: header, AUX fields, `MODULE_AUX` (before keyspace), `FUNCTION2`, then per
database `SELECTDB`, `RESIZEDB` and the keys, then `MODULE_AUX` (after keyspace), `EOF` and the checksum
(`rdbSaveRio` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L1673)).

## Magic Number

The file starts off with the magic string "REDIS". This is a quick sanity check to know we are dealing with a redis rdb file.

```
52 45 44 49 53  # "REDIS"
```

## RDB Version Number

The next 4 bytes store the version number of the rdb format. The 4 bytes are interpreted as ASCII characters and then converted to an integer using string to integer conversion.

```
30 30 31 33 # "0013" => Version 13
```

Redis refuses files with a version higher than the one it writes. The newest format is version 13, written by Redis 8.6.
*rdb-rs* accepts versions 1 to 13.

## Op Codes

Each part after the initial header is introduced by a special op code. Op codes share the byte space with the
[Value Type](#value-type) flag: a byte that is not one of the op codes below is a value type and starts a key-value pair.
The available op codes are:

| Byte | Dec | Name            | Since RDB | Payload / Description |
| ---- | --- | --------------- | --------- | --------------------- |
| 0xFF | 255 | EOF             | 1         | End of the RDB file, followed by the [CRC64 checksum](#crc64-checksum) (RDB >= 5) |
| 0xFE | 254 | SELECTDB        | 1         | [Database Selector](#database-selector): length-encoded db number |
| 0xFD | 253 | EXPIRETIME      | 1         | Expire time in seconds: 4 bytes, see [Key Expiry Timestamp](#key-expiry-timestamp) |
| 0xFC | 252 | EXPIRETIME_MS   | 3         | Expire time in milliseconds: 8 bytes, see [Key Expiry Timestamp](#key-expiry-timestamp) |
| 0xFB | 251 | RESIZEDB        | 7         | Two length-encoded hash table sizes, see [Resizedb information](#resizedb-information) |
| 0xFA | 250 | AUX             | 7         | Two strings (key, value), see [Auxiliary fields](#auxiliary-fields) |
| 0xF9 | 249 | FREQ            | 9         | 1 raw byte LFU counter, key prefix, see [LRU and LFU information](#lru-and-lfu-information) |
| 0xF8 | 248 | IDLE            | 9         | Length-encoded LRU idle time in seconds, key prefix, see [LRU and LFU information](#lru-and-lfu-information) |
| 0xF7 | 247 | MODULE_AUX      | 9         | Module auxiliary data, see [Module auxiliary data](#module-auxiliary-data) |
| 0xF6 | 246 | FUNCTION_PRE_GA | 10        | Function library in the Redis 7.0 release candidate format, see [Functions](#functions) |
| 0xF5 | 245 | FUNCTION2       | 10        | One string: function library code, see [Functions](#functions) |
| 0xF4 | 244 | SLOT_INFO       | 12        | Three length-encoded ints: slot id, slot size, expires slot size, see [Slot information](#slot-information) |
| 0xF3 | 243 | KEY_META        | 13        | Module key metadata, key prefix in the type byte position, see [Key metadata](#key-metadata) |

Source: `#define RDB_OPCODE_*` in [`src/rdb.h` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.h#L87-L100).

## Database Selector

A Redis instance can have multiple databases.

A single byte `0xFE` flags the start of the database selector. After this byte, a variable length field indicates the database number. See the section [Length Encoding](#length-encoding) to understand how to read this database number.

## Resizedb information

This op code was introduced in RDB version 7.

It encodes two values to speed up RDB loading by avoiding additional resizes and rehashing.
The op code is followed by two [length-encoded](#length-encoding) integers indicating:

* Database hash table size
* Expiry hash table size

## Auxiliary fields

This op code was introduced in RDB version 7.

The op code is followed by two [Redis Strings](#string-encoding), representing the key and value of a setting.
Unknown fields should be ignored by a parser.

Redis 8.6 writes the following settings (`rdbSaveInfoAuxFields` in `src/rdb.c` at 8.6.7):

* `redis-ver`: The Redis Version that wrote the RDB
* `redis-bits`: Bit architecture of the system that wrote the RDB, either 32 or 64
* `ctime`: Creation time of the RDB
* `used-mem`: Used memory of the instance that wrote the RDB
* `repl-stream-db`, `repl-id`, `repl-offset`: Replication information, only when the RDB is saved with replication info (e.g. for a full sync, or by an instance that is or was a replica)
* `aof-base`: Whether the RDB is the base of a multi-part AOF
* `cluster-asm-task`: Only written when cluster mode is enabled

Older Redis versions wrote other fields as well (for example `aof-preamble`, or `lua` holding Lua scripts in Redis 5 and 6).

## Slot information

This op code was introduced in RDB version 12 (Redis 7.4). Redis only writes it in cluster mode, once before the
first key of each hash slot.

```
F4                          # SLOT_INFO
$length-encoded-int         # slot id
$length-encoded-int         # number of keys in the slot
$length-encoded-int         # number of keys with an expiry in the slot
```

Redis ignores it when loading without cluster mode. *rdb-rs* skips it.

Source: `rdbLoadRioWithLoadingCtx` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L3778).

## Functions

`FUNCTION2` (`0xF5`) was introduced in RDB version 10 (Redis 7.0). It is followed by exactly one
[string](#string-encoding) holding the source code of a function library (including its `#!lua name=...`
shebang line). There is one `FUNCTION2` op code per library, written before the first database selector.
*rdb-rs* skips it.

`FUNCTION_PRE_GA` (`0xF6`) is the format of the Redis 7.0 release candidates (originally named `FUNCTION`).
Redis 7.0 GA and later refuse to load it and its layout is not described by the Redis source, so it cannot be
skipped. *rdb-rs* stops with an "unsupported" error.

Source: `rdbFunctionLoad` and `rdbLoadRioWithLoadingCtx` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L3921-L3930).

## Module auxiliary data

`MODULE_AUX` (`0xF7`) was introduced in RDB version 9 (Redis 5.0). It carries data a module stores outside of the
keyspace. It may appear before the first database selector and/or after the last key.

```
F7                          # MODULE_AUX
$length-encoded-int         # module id (64 bit, see Module Id below)
$length-encoded-int         # when_opcode, always 2 (RDB_MODULE_OPCODE_UINT)
$length-encoded-int         # when: 1 = before the keyspace, 2 = after the keyspace
$module-value-stream        # (opcode, payload)* terminated by opcode 0, see Module Value Stream
```

The data is self-describing, so it can be skipped without the module: read the three lengths, check that
`when_opcode` is 2, then skip the [module value stream](#module-value-stream) up to and including its `EOF`
opcode. *rdb-rs* skips it this way.

Source: `rdbSaveSingleModuleAux` and `rdbLoadRioWithLoadingCtx` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L3868-L3920).

## Key Value Pairs

After the database selector, the file contains a sequence of key value pairs.

Each key value pair has these parts:

* Optional prefixes: [Key Expiry Timestamp](#key-expiry-timestamp), [LRU and LFU information](#lru-and-lfu-information) and [Key metadata](#key-metadata).
* 1 byte flag indicating the value type.
* The key, encoded as a Redis String. See [String Encoding](#string-encoding).
* The value, encoded according to the value type. See [Value Encoding](#value).

Redis writes the prefixes in this order (`rdbSaveKeyValuePair` in `src/rdb.c` at 8.6.7):

```
[FC $8-byte-ms]             # EXPIRETIME_MS, keys with an expiry
[F8 $length-encoded-int]    # IDLE, when maxmemory-policy is an LRU policy
[F9 $byte]                  # FREQ, when maxmemory-policy is an LFU policy
[F3 $key-metadata]          # KEY_META, keys carrying module metadata (RDB 13)
$value-type
$string-encoded-key
$encoded-value
```

`EXPIRETIME`, `EXPIRETIME_MS`, `IDLE` and `FREQ` are loaded like any other op code: the reader handles the op code and
then reads the next op code, so they apply to the next key that follows. `KEY_META` is different: it takes the place of
the type byte, and the actual value type byte follows directly after the metadata.

### Key Expiry Timestamp

This section starts with a one byte flag.
This flag is either:

* `0xFD`: The following expire value is specified in seconds. The following 4 bytes represent the Unix timestamp as a 32 bit integer, stored in the host byte order of the writer (little endian in practice; Redis reads it as a signed `int32_t`).
* `0xFC`: The following expire value is specified in milliseconds. The following 8 bytes represent the Unix timestamp as a 64 bit little endian integer. Before RDB version 9, these 8 bytes were written in host byte order, so files written by big endian machines differ.

Redis versions since 2.6 (RDB 3+) only write `0xFC`.

During the import process, keys that have expired must be discarded. *rdb-rs* reports the expiry in milliseconds and leaves the decision to the caller.

### LRU and LFU information

Introduced in RDB version 9 (Redis 5.0), written depending on the `maxmemory-policy` of the instance:

* `0xF8` (`IDLE`): the idle time of the key in seconds, [length-encoded](#length-encoding).
* `0xF9` (`FREQ`): one raw byte holding the LFU access counter.

*rdb-rs* reads and discards both.

### Key metadata

`KEY_META` (`0xF3`) was introduced in RDB version 13 (Redis 8.6). It is written only for keys carrying metadata
attached by modules, after any expiry/`IDLE`/`FREQ` prefix, in the position of the value type byte.

```
F3                          # KEY_META
$length-encoded-int         # number of metadata classes (at most 7)
  4 bytes                   # class spec: uint32 in host byte order (little endian in practice)
  $module-value-stream      # (opcode, payload)* terminated by opcode 0, see Module Value Stream
  ...                       # repeated for every class
$value-type                 # the real value type byte follows immediately
$string-encoded-key
$encoded-value
```

The class spec packs a 4 character class name into bits 31..8 (6 bits per character, first character in the most
significant bits, charset `A-Z a-z 0-9 _ -`), a version into bits 7..3 and flags into bits 2..0 (bit 0 =
`ALLOW_IGNORE`). Redis skips classes it does not know by skipping the module value stream, so a parser can skip the
whole block without understanding the metadata. *rdb-rs* does exactly that.

Source: `rdbResolveKeyType` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L1983)
and `rdbLoadKeyMetadata` in [`src/keymeta.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/keymeta.c).

### Value Type

A one byte flag indicates encoding used to save the Value.

| Type | Name                         | Since RDB | Encoding | rdb-rs |
| ---- | ---------------------------- | --------- | -------- | ------ |
| `0`  | STRING                       | 1  | [String Encoding](#string-encoding) | decoded |
| `1`  | LIST                         | 1  | [List Encoding](#list-encoding) | decoded |
| `2`  | SET                          | 1  | [Set Encoding](#set-encoding) | decoded |
| `3`  | ZSET                         | 1  | [Sorted Set Encoding](#sorted-set-encoding) | decoded |
| `4`  | HASH                         | 1  | [Hash Encoding](#hash-encoding) | decoded |
| `5`  | ZSET_2                       | 8  | [Sorted Set 2 Encoding](#sorted-set-2-encoding) | decoded |
| `6`  | MODULE_PRE_GA (was MODULE)   | 8  | Redis 4.0 release candidate module format, see [Module Encoding](#module-encoding) | unsupported (error) |
| `7`  | MODULE_2                     | 8  | [Module Encoding](#module-encoding) | skipped |
| `8`  | —                            | —  | unused | error |
| `9`  | HASH_ZIPMAP                  | 2  | [Zipmap Encoding](#zipmap-encoding) | decoded |
| `10` | LIST_ZIPLIST                 | 2  | [Ziplist Encoding](#ziplist-encoding) | decoded |
| `11` | SET_INTSET                   | 2  | [Intset Encoding](#intset-encoding) | decoded |
| `12` | ZSET_ZIPLIST                 | 2  | [Sorted Set in Ziplist Encoding](#sorted-set-in-ziplist-encoding) | decoded |
| `13` | HASH_ZIPLIST                 | 4  | [Hashmap in Ziplist Encoding](#hashmap-in-ziplist-encoding) | decoded |
| `14` | LIST_QUICKLIST               | 7  | [Quicklist Encoding](#quicklist-encoding) | decoded |
| `15` | STREAM_LISTPACKS             | 9  | [Stream Encoding](#stream-encoding) | skipped |
| `16` | HASH_LISTPACK                | 10 | [Hash in Listpack Encoding](#hash-in-listpack-encoding) | decoded |
| `17` | ZSET_LISTPACK                | 10 | [Sorted Set in Listpack Encoding](#sorted-set-in-listpack-encoding) | decoded |
| `18` | LIST_QUICKLIST_2             | 10 | [Quicklist 2 Encoding](#quicklist-2-encoding) | decoded |
| `19` | STREAM_LISTPACKS_2           | 10 | [Stream Encoding](#stream-encoding) | skipped |
| `20` | SET_LISTPACK                 | 11 | [Set in Listpack Encoding](#set-in-listpack-encoding) | decoded |
| `21` | STREAM_LISTPACKS_3           | 11 | [Stream Encoding](#stream-encoding) | skipped |
| `22` | HASH_METADATA_PRE_GA         | 12 | [Hash with Field TTLs](#hash-with-field-ttls) | decoded (field TTLs dropped) |
| `23` | HASH_LISTPACK_EX_PRE_GA      | 12 | [Hash with Field TTLs](#hash-with-field-ttls) | decoded (field TTLs dropped) |
| `24` | HASH_METADATA                | 12 | [Hash with Field TTLs](#hash-with-field-ttls) | decoded (field TTLs dropped) |
| `25` | HASH_LISTPACK_EX             | 12 | [Hash with Field TTLs](#hash-with-field-ttls) | decoded (field TTLs dropped) |
| `26` | STREAM_LISTPACKS_4           | 13 | [Stream Encoding](#stream-encoding) | skipped |

"Skipped" means rdb-rs walks the value byte by byte so the rest of the file can still be parsed, but does not report it.

Source: `#define RDB_TYPE_*` in [`src/rdb.h` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.h#L55-L81).

### Key

The key is simply encoded as a Redis string. See the section [String Encoding](#string-encoding) to learn how the key is encoded.

### Value

The value is parsed according to the previously read [Value Type](#value-type)

## Encodings

### Length Encoding

Length encoding is used to store the length of the next object in the stream. Length encoding is a variable byte encoding designed to use as few bytes as possible.

This is how length encoding works :
Read one byte from the stream, compare the two most significant bits:

| Bits | How to parse |
| ---- | ------------ |
| `00` | The next 6 bits represent the length |
| `01` | Read one additional byte. The combined 14 bits represent the length (big endian: the 6 bits are the high bits) |
| `10` | The whole first byte selects the size, see below |
| `11` | The next object is encoded in a special format. The remaining 6 bits indicate the format. May be used to store numbers or Strings, see [String Encoding](#string-encoding) |

When the two most significant bits are `10`, the first byte must be one of:

| First byte | How to parse |
| ---------- | ------------ |
| `0x80`     | The next 4 bytes are the length as a 32 bit **big endian** unsigned integer |
| `0x81`     | The next 8 bytes are the length as a 64 bit **big endian** unsigned integer (RDB 8+) |

Any other `10xxxxxx` byte is invalid. Redis writes the shortest form, so `0x81` appears whenever a value exceeds
2^32 - 1. This is common in modern files: millisecond stream IDs, hash field TTLs and signed module integers
(stored as their unsigned 64 bit two's complement) are all stored as length-encoded integers.

As a result of this encoding:

* Numbers up to and including 63 can be stored in 1 byte
* Numbers up to and including 16383 can be stored in 2 bytes
* Numbers up to 2^32 - 1 can be stored in 5 bytes
* Numbers up to 2^64 - 1 can be stored in 9 bytes

**Examples:**

```
0A                          # 10
42 BC                       # 0x02BC = 700
80 00 01 11 70              # 0x00011170 = 70000
81 00 00 01 8B CF E5 68 00  # 0x0000018BCFE56800 = 1700000000000
```

Source: `rdbSaveLen` / `rdbLoadLenByRef` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L159-L235).

### String Encoding

Redis Strings are binary safe - which means you can store anything in them. They do not have any special end-of-string token. It is best to think of Redis Strings as a byte array.

There are three types of Strings in Redis:

* Length prefixed strings
* An 8, 16 or 32 bit integer
* A LZF compressed string

All blobs embedded in a value (ziplists, listpacks, intsets, ...) are stored as such strings, so they may be LZF
compressed as well.

#### Length Prefixed String

Length prefixed strings are quite simple. The length of the string in bytes is first encoded using [Length Encoding](#length-encoding). After this, the raw bytes of the string are stored.

#### Integers as String

First read the section [Length Encoding](#length-encoding), specifically the part when the first two bits are `11`. In this case, the remaining 6 bits are read.

If the value of those 6 bits is:

* `0` indicates that an 8 bit signed integer follows
* `1` indicates that a 16 bit signed little endian integer follows
* `2` indicates that a 32 bit signed little endian integer follows

The string value is the decimal representation of the integer, e.g. `C1 39 30` is the string `"12345"`.

#### Compressed Strings

First read the section [Length Encoding](#length-encoding), specifically the part when the first two bits are `11`. In this case, the remaining 6 bits are read.
If the value of those 6 bits is 3, it indicates that a compressed string follows.

The compressed string is read as follows:

* The compressed length `clen` is read from the stream using [Length Encoding](#length-encoding)
* The uncompressed length is read from the stream using [Length Encoding](#length-encoding)
* The next `clen` bytes are read from the stream
* Finally, these bytes are decompressed using LZF algorithm

### List Encoding

A Redis list is represented as a sequence of strings.

* First, the size of the list `size` is read from the stream using [Length Encoding](#length-encoding)
* Next, `size` strings are read from the stream using [String Encoding](#string-encoding)
* The list is then re-constructed using these Strings

### Set Encoding

Sets are encoded exactly like lists.

### Sorted Set Encoding

* First, the size of the sorted set `size` is read from the stream using [Length Encoding](#length-encoding)
* Next, `size` pairs of the value and its score are read from the stream.
* The value is [string encoded](#string-encoding)
* The next byte specifies the length of the encoded score (as an unsigned integer). This byte has 3 special meanings:
    * `253`: not a number. Don't read additional bytes
    * `254`: positive infinity. Don't read additional bytes
    * `255`: negative infinity. Don't read additional bytes
* Read that many bytes from the stream, representing the score as an ASCII-encoded float. Use String-to-float conversion to get the actual double value.

*Note: Depending on the value you may loose precision. Redis saves the score as a double.*

*Note: It's not guaranteed that the set is sorted already.*

**Example:**

```
04
01 63 12 34 2E 30 31 39 39 39 39 39 39 39 39 39 39 39 39 39 36
01 64 FE
01 61 12 33 2E 31 38 39 39 39 39 39 39 39 39 39 39 39 39 39 39
01 65 FF
```

* First read the size of the sorted set: `04` = `4` (decimal)
* Next, read the first member, [string encoded](#string-encoding). It has a length of `01` = `1` (decimal). Read one byte: `63` = `c` (ASCII).
* Then read the next byte: `12` = `18` (decimal). This is the lenght of the following ASCII-encoded score.
* Read 18 bytes as ASCII values: `4.0199999999999996`. This could be parsed into a double value if necessary.
* Read one byte: `01` = `1`, the length of the next member.  Read that byte: `64` = `d` (ASCII)
* Read one more byte: `FE` = `254` (decimal). This means the score is positive infinity.
* Read the next byte: `01`. Again, the length of the next member. Read that byte: `61` = `a`.
* Read the next byte: `12` = `18` (decimal). Read the next 18 bytes and interpret them as ASCII: `""3.1899999999999999`
* Read one byte: `01`. Again, the length of the next member. Read that byte: `65` = `e`.
* Read one more byte: `FF` = `255` (decimal). This means the score is negative infinity.

The resulting sorted set will be:

```
{ "e" => "-inf", "a" => "3.189999999999999", "c" => "4.0199999999999996", "d" => "+inf" }
```

### Sorted Set 2 Encoding

Type `5` (`ZSET_2`) was introduced in RDB version 8 (Redis 4.0) and replaces type `3` for sorted sets that are too
large for the compact encoding. It is identical to the [Sorted Set Encoding](#sorted-set-encoding), except that each
score is stored as an 8 byte IEEE 754 double in **little endian** byte order, without a length byte. Infinities and NaN
use their IEEE 754 bit patterns.

* `size`, [length-encoded](#length-encoding)
* `size` times: member ([string encoded](#string-encoding)), score (8 bytes, little endian double)

Redis writes the members in descending score order, but a parser should not rely on it.

**Example:**

```
05 01 7A 01 01 6D 00 00 00 00 00 00 F8 3F
```

* `05`: value type `ZSET_2`, `01 7A`: key `"z"`
* `01`: one member
* `01 6D`: member `"m"`
* `00 00 00 00 00 00 F8 3F`: `0x3FF8000000000000` = `1.5`

Source: `rdbLoadObject` and `rdbLoadBinaryDoubleValue` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L2340-L2375).

### Hash Encoding

* First, the `size` of the hash is read from the stream using [Length Encoding](#length-encoding)
* Next ` 2 * size ` strings are read from the stream using [String Encoding](#string-encoding) (alternate strings are key and values)

**Example:**

```
2 us washington india delhi
```

represents the map

```
{"us" => "washington", "india" => "delhi"}
```

### Zipmap Encoding

A Zipmap is a hashmap that has been serialized to a string. In essence, the key value pairs are stored sequentially. Looking up a key in this structure is O(N). This structure is used instead of a dictionary when the number of key value pairs are small.

Zipmaps are only written by Redis versions before 2.6 (RDB < 4); newer versions convert them on load.

To parse a zipmap, first a string is read from the stream using [String Encoding](#string-encoding).
The contents of this string represent the zipmap.

The structure of a zipmap within this string is as follows:

```
<zmlen><len>"foo"<len><free>"bar"<len>"hello"<len><free>"world"<zmend>
```

* `zmlen`: 1 byte that holds the size of the zip map. If it is greater than or equal to 254, value is not used. You will have to iterate the entire zip map to find the length.
* `len`: the length of the following string, which can be either a key or a value. This length is stored in either 1 byte or 5 bytes (yes, it differs from [Length Encoding](#length-encoding) described above). If the first byte is between 0 and 252, that is the length of the zipmap. If the first byte is 253, then the next 4 bytes read as an unsigned integer represent the length of the zipmap. 254 and 255 are invalid values for this field.
* `free` : This is always 1 byte, and indicates the number of free bytes _after_ the value. For example, if the value of a key is "America" and its get updated to "USA", 4 free bytes will be available.
* `zmend` : Always `255`. Indicates the end of the zipmap.

**Example:**

`18 02 06 4D 4B 44 31 47 36 01 00 32 05 59 4E 4E 58 4b 04 00 46 37 54 49 FF ..`

* Start by decoding this using [String Encoding](#string-encoding). You will notice that `0x18` (`24` in decimal) is the length of the string. Accordingly, we will read the next 24 bytes i.e. up to `FF`
* Now, we are parsing the string starting at `02 06... ` using the [Zipmap Encoding](#zipmap-encoding)
* `02` is the number of entries in the hashmap.
* `06` is the length of the next string. Since this is less than 254, we don't have to read any additional bytes
* We read the next 6 bytes i.e. `4d 4b 44 31 47 36` to get the key "MKD1G6"
* `01` is the length of the next string, which would be the value
* `00` is the number of free bytes
* We read the next 1 byte, which is `0x32`. Thus, we get our value `"2"`
* In this case, the free bytes is `0`, so we don't skip anything
* `05` is the length of the next string, in this case a key.
* We read the next 5 bytes `59 4e 4e 58 4b`, to get the key `"YNNXK"`
* `04` is the length of the next string, which is a value
* `00` is the number of free bytes after the value
* We read the next 4 bytes i.e. `46 37 54 49` to get the value `"F7TI"`
* Finally, we encounter `FF`, which indicates the end of this zip map
* Thus, this zip map represents the hash `{"MKD1G6" => "2", "YNNXK" => "F7TI"}`

### Ziplist Encoding

A Ziplist is a list that has been serialized to a string. In essence, the elements of the list are stored sequentially along with flags and offsets to allow efficient traversal of the list in both directions.

Ziplists are written by Redis versions before 7.0 (RDB < 10). Redis 7.0 replaced them with [listpacks](#listpack-encoding) but still loads them.

To parse a ziplist, first a string is read from the stream using [String Encoding](#string-encoding).
The contents of this string represent the ziplist.

The structure of a ziplist within this string is as follows:

```
<zlbytes><zltail><zllen><entry><entry><zlend>
```

* `zlbytes`: a 4 byte unsigned integer representing the total size in bytes of the ziplist. The 4 bytes are in little endian format - the least significant bit comes first.
* `zltail`: a 4 byte unsigned integer in little endian format. It represents the offset to the tail (i.e. last) entry in the ziplist
* `zllen`: This is a 2 byte unsigned integer in little endian format. It represents the number of entries in this ziplist
* `entry`: An entry represents an element in the ziplist. Details below
* `zlend`: Always `255`. It represents the end of the ziplist.

Each entry in the ziplist has the following format :

```
<length-prev-entry><special-flag><raw-bytes-of-entry>
```

`length-prev-entry`: stores the length of the previous entry, or 0 if this is the first entry. This allows easy traversal of the list in the reverse direction. This length is stored in either 1 byte or in 5 bytes. If the first byte is less than or equal to 253, it is considered as the length. If the first byte is 254, then the next 4 bytes are used to store the length. The 4 bytes are read as an unsigned integer.

`special-flag`: This flag indicates whether the entry is a string or an integer. It also indicates the length of the string, or the size of the integer.
The various encodings of this flag are shown below:

| Bytes | Length | Meaning |
| ----- | ------ | ------- |
| `00pppppp` | 1 byte | String value with length less than or equal to 63 bytes (6 bits) |
| `01pppppp` `qqqqqqqq` | 2 bytes | String value with length less than or equal to 16383 bytes (14 bits, big endian) |
| `10000000` `<4 bytes>` | 5 bytes | Next 4 bytes contain a big endian unsigned int. String value with length greater than or equal to 16384 bytes |
| `11000000` | 3 bytes | Integer encoded as 16 bit signed (2 bytes) |
| `11010000` | 5 bytes | Integer encoded as 32 bit signed (4 bytes) |
| `11100000` | 9 bytes | Integer encoded as 64 bit signed (8 bytes) |
| `11110000` | 4 bytes | Integer encoded as 24 bit signed (3 bytes) |
| `11111110` | 2 bytes | Integer encoded as 8 bit signed (1 byte) |
| `1111xxxx` | 1 byte | Immediate 4 bit integer: `xxxx` is between `0001` and `1101`, the value is `xxxx - 1` (0 to 12) |

All integers are stored in little endian byte order. The 8 bit and immediate encodings were added in RDB version 6.

`raw-byte`: After the special flag, the raw bytes of entry follow. The number of bytes was previously determined as part of the special flag.

**Example:**

```
23 23 00 00 00 1E 00 00 00 04 00 00 E0 FF FF FF FF FF
FF FF 7F 0A D0 FF FF 00 00 06 C0 FC 3F 04 C0 3F 00 FF ...
```

* Start by decoding this using [String Encoding](#string-encoding). `23` is the length of the string (35 in decimal), therefore we will read the next 35 bytes till `ff`
* Now, we are parsing the string starting at `23 00 00 ...` using [Ziplist encoding](#ziplist-encoding)
* The first 4 bytes `23 00 00 00` represent the total length in bytes of this ziplist. Notice that this is in little endian format
* The next 4 bytes `1e 00 00 00` represent the offset to the tail entry. `1E` = 30 (in decimal), and this is a 0 based offset. 0th position = `23`, 1st position = `00` and so on. It follows that the last entry starts at `04 c0 3f 00 ..`
* The next 2 bytes `04 00` represent the number of entries in this list as a little endian 16 bit integer. `04 00 = 4` in decimal.
* From now on, we start reading the entries
* `00` represents the length of previous entry. `0` indicates this is the first entry.
* `E0` is the special flag. Since it starts with the bit pattern `1110____`, we read the next 8 bytes as an integer. This is the first entry of the list.
* We now start the second entry
* `0A` is the length of the previous entry. `0A` = `10` in decimal. 10 bytes = 1 byte for prev. length + 1 byte for special flag + 8 bytes for integer.
* `D0` is the special flag. Since it starts with the bit pattern `1101____`, we read the next 4 bytes as an integer. This is the second entry of the list
* We now start the third entry
* `06` is the length of previous entry. 6 bytes = 1 byte for prev. length + 1 byte for special flag + 4 bytes for integer
* `C0` is the special flag. Since it starts with the bit pattern `1100____`, we read the next 2 bytes as an integer. This is the third entry of the list
* We now start the last entry
* `04` is length of previous entry
* `C0` indicates a 2 byte number
* We read the next 2 bytes, which gives us our fourth entry
* Finally, we encounter `FF`, which tells us we have consumed all elements in this ziplist.
* Thus, this ziplist stores the values `[0x7fffffffffffffff, 65535, 16380, 63]`

### Intset Encoding

An Intset is a sorted array of integers. An intset is used when all the elements of the set are integers. An Intset has support for up to 64 bit integers. As an optimization, if the integers can be represented in fewer bytes, the array of integers will be constructed from 16 bit or 32 bit integers. When a new element is inserted, the implementation takes care to upgrade if necessary.

Since the array is sorted, the numbers in this set will always be sorted.

An Intset has an external interface of a Set.

To parse an Intset, first a string is read from thee stream using [String Encoding](#string-encoding).
The contents of this string represent the Intset.

Within this string, the Intset has a very simple layout :

```
<encoding><length-of-contents><contents>
```

* `encoding`: is a 32 bit little endian unsigned integer. It has 3 possible values - 2, 4 or 8. It indicates the size in bytes of each integer stored in contents. And yes, this is wasteful - we could have stored the same information in 2 bits.
* `length-of-contents`: is a 32 bit little endian unsigned integer, and indicates the number of integers in the contents array
* `contents`: is an array of `length-of-contents * encoding` bytes. It contains the sorted signed little endian integers

**Example**

`14 04 00 00 00 03 00 00 00 FC FF 00 00 FD FF 00 00 FE FF 00 00 ...`

* Start by decoding this using "String Encoding". `14` is the length of the string, therefore we will read the next 20 bytes till `00`
* Now, we start interpreting the string starting at `04 00 00 ... `
* The first 4 bytes `04 00 00 00` is the encoding. Since this evaluates to 4, we know we are dealing with 32 bit integers
* The next 4 bytes `03 00 00 00` is the length of contents. So, we know we are dealing with 3 integers, each 4 byte long
* From now on, we read in groups of 4 bytes, and convert it into a signed integer
* Thus, our intset looks like `[0x0000FFFC, 0x0000FFFD, 0x0000FFFE]`. Notice that the integers are in little endian format i.e. least significant bit came first.

### Sorted Set in Ziplist Encoding

A sorted set in ziplist encoding is stored just like the [Ziplist](#ziplist-encoding) described above. Each element in the sorted set is followed by its score in the ziplist.

**Example**

`['Manchester City', 1, 'Manchester United', 2, 'Tottenham', 3]`

As you see, the scores follow each element.

### Hashmap in Ziplist Encoding

In this, key=value pairs of a hashmap are stored as successive entries in a ziplist.

Note: This was introduced in RDB version 4. This deprecates the [zipmap encoding](#zipmap-encoding) that was used in earlier versions.

**Example**

`{"us" => "washington", "india" => "delhi"}`

is stored in a ziplist as :

`["us", "washington", "india", "delhi"]`

### Quicklist Encoding

RDB Version 7 introduced a new variant of list encoding: Quicklist. It is written by Redis 3.2 up to 6.2; Redis 7.0
replaced it with [Quicklist 2](#quicklist-2-encoding).

Quicklist is a linked list of ziplists. Quicklist combines the memory efficiency of small ziplists with the extensibility of a linked list allowing us to create space-efficient lists of any length.

The structure of a quicklist is as follows:

```
<len><ziplist><ziplist>...
```

* `len`: This is the number of nodes of the linked list, [length-encoded](#length-encoding)
* `ziplist`: A string that wraps a ziplist, parse it with [Ziplist encoding](#ziplist-encoding)

A complete list needs to be constructed from all elements of all ziplists.

**Example**:

```
01 1F 1F 00 00 00 17 00 00 00 02 00 00 0B 6F 6E 65 2D 65 6C 65 6D 65 6E 74 0D 05 65 6C 65 6D 32 FF
```

* Start by reading the first byte in order to get the list length in [length-encoding](#length-encoding). In this case it is `01`, so the list contains just one node.
* What follows is a ziplist containing all elements for this node.
* Proceed by reading the ziplist as specified in [Ziplist Encoding](#ziplist-encoding).
* Read the blob, [string-encoded](#string-encoding)
* For this read the length, `0x1F`, so 31 bytes up to and including `FF`
* Read these 31 bytes.
* Read the number of bytes of the ziplist as a 32 bit unsigned integer: `1F 00 00 00` = `31` (decimal).
* Read the tail offset as a 32 bit unsigned integer: `17 00 00 00` = `23` (decimal). It's zero-based, so this points to `0D 05 65 6C ...`
* Read the length of the ziplist as a 16 bit unsigned integer: `02 00` = `2` (decimal).
* Now read two ziplist entries as follows:
* Read the next byte: `00`, as this is the first item and has no predecessors item.
* Read the next byte: `0B` = `11` (decimal). As this starts with the bit pattern `00` it is treated as the length.
* Read 11 bytes from the stream: `6F 6E 65 2D 65 6C 65 6D 65 6E 74` = `"one-element"` (ASCII)
* This is the first element.
* Proceed with the second item.
* Read one byte: `0D` = `13` (decimal). This is the length of the previous item (length of 11 + flag + prev-length)
* Read the next byte: `05`. This is the size of the following string.
* Read the next 5 bytes: `65 6C 65 6D 32` = `elem2`. This is the second list element.
* Read the next byte: `FF`. This marks the end of the ziplist (as we read 2 elements as determined before)
* Thus, the Quicklist stores the list `["one-element", "elem2"]`.

### Listpack Encoding

A listpack is the successor of the ziplist, used by Redis 5.0+ for streams and by Redis 7.0+ (RDB 10) for all compact
hashes, sorted sets, lists and (Redis 7.2+) sets. Like a ziplist it is stored as a single [string](#string-encoding)
(possibly LZF compressed). Unlike a ziplist, each entry stores its own length *after* the data instead of the length of
the previous entry.

```
<total-bytes><num-elements><entry><entry>...<end>
```

* `total-bytes`: 4 byte little endian unsigned integer, the size of the whole listpack including header and end byte.
* `num-elements`: 2 byte little endian unsigned integer, the number of entries. `65535` means the count is unknown
  (65535 or more entries) and the listpack must be walked to count them.
* `end`: Always `0xFF`.

Each entry is

```
<encoding-type><entry-data><backlen>
```

The first byte of `encoding-type` determines the entry type:

| First byte | Entry bytes (without backlen) | Meaning |
| ---------- | ----------------------------- | ------- |
| `0xxxxxxx` | 1 | 7 bit unsigned integer `xxxxxxx` (0 to 127) |
| `10xxxxxx` | 1 + len | String, `xxxxxx` is the length (up to 63 bytes) |
| `110xxxxx` `yyyyyyyy` | 2 | 13 bit signed integer, `xxxxx` are the high bits; values >= 4096 are negative (subtract 8192). Range -4096 to 4095 |
| `1110xxxx` `yyyyyyyy` | 2 + len | String, 12 bit length (`xxxx` high bits, up to 4095 bytes) |
| `11110000` `<4 bytes>` | 5 + len | String, 32 bit little endian length |
| `11110001` `<2 bytes>` | 3 | 16 bit signed integer, little endian |
| `11110010` `<3 bytes>` | 4 | 24 bit signed integer, little endian |
| `11110011` `<4 bytes>` | 5 | 32 bit signed integer, little endian |
| `11110100` `<8 bytes>` | 9 | 64 bit signed integer, little endian |
| `11110101` - `11111110` | — | unused, invalid |
| `11111111` | — | `0xFF`: end of the listpack, not an entry |

Redis stores a value as an integer whenever the string is the canonical decimal representation of a 64 bit signed
integer, picking the smallest encoding. Readers that want the original value render integers in decimal.

`backlen` stores the number of bytes of `encoding-type` plus `entry-data` (the "Entry bytes" column) so the listpack
can be walked backwards. It is a variable length integer, read right to left, using 7 bits per byte. Its size only
depends on that length `l`:

| `l` | backlen size |
| --- | ------------ |
| 0 - 127 | 1 byte |
| 128 - 16382 | 2 bytes |
| 16383 - 2097150 | 3 bytes |
| 2097151 - 268435454 | 4 bytes |
| 268435455 and more | 5 bytes |

A forward reader can compute the backlen size from the entry and skip it.

**Example:**

A hash `{"f" => "v", "n" => "1000"}` stored as type `16` ([Hash in Listpack](#hash-in-listpack-encoding)):

```
10 01 68 13
13 00 00 00 04 00
81 66 02
81 76 02
81 6E 02
C3 E8 02
FF
```

* `10`: value type `16`, `01 68`: key `"h"`
* `13`: the listpack is a 19 byte string
* `13 00 00 00`: total bytes = 19; `04 00`: 4 entries
* `81 66 02`: `10000001` is a string of length 1, `66` = `"f"`, `02` is the backlen (2 bytes of encoding + data)
* `81 76 02`: string `"v"`; `81 6E 02`: string `"n"`
* `C3 E8 02`: `110 00011` `E8` is a 13 bit integer `0x3E8` = `1000`, backlen 2
* `FF`: end of the listpack

The 13 bit integer `-1` would be `DF FF` (`0x1FFF` = 8191, minus 8192).

Source: `lpEncodeIntegerGetType`, `lpEncodeBacklen` and the `LP_ENCODING_*` defines in [`src/listpack.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/listpack.c#L27-L83).

### Hash in Listpack Encoding

Type `16` (`HASH_LISTPACK`, RDB 10). A string containing a [listpack](#listpack-encoding) whose entries alternate field
and value: `[field1, value1, field2, value2, ...]`. It replaces [Hashmap in Ziplist](#hashmap-in-ziplist-encoding).

### Sorted Set in Listpack Encoding

Type `17` (`ZSET_LISTPACK`, RDB 10). A string containing a [listpack](#listpack-encoding) whose entries alternate member
and score: `[member1, score1, member2, score2, ...]`, sorted by score. Scores are stored as strings (e.g. `"1.5"`,
`"inf"`) or, when the score is integral, as listpack integers. It replaces
[Sorted Set in Ziplist](#sorted-set-in-ziplist-encoding).

### Set in Listpack Encoding

Type `20` (`SET_LISTPACK`, RDB 11, Redis 7.2). A string containing a [listpack](#listpack-encoding) with one entry per
member. It is used for small sets with non-integer members; integer-only sets still use the
[Intset Encoding](#intset-encoding).

Source for the three listpack types: `rdbLoadObject` (the `RDB_TYPE_*_LISTPACK` cases, validated with
`lpValidateIntegrityAndDups` using a tuple length of 2, 2 and 1) in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L2913-L3057).

### Quicklist 2 Encoding

Type `18` (`LIST_QUICKLIST_2`) was introduced in RDB version 10 (Redis 7.0). Like the
[Quicklist](#quicklist-encoding) it is a list of nodes, but every node carries a container type, and packed nodes are
listpacks instead of ziplists:

```
<len>                       # number of nodes, length-encoded
<container><data>           # repeated len times
```

* `container`: [length-encoded](#length-encoding) node type:
    * `1` (`PLAIN`): `data` is a [string](#string-encoding) that is a single list element. Redis uses plain nodes for elements too large to pack (bigger than `list-max-listpack-size` allows).
    * `2` (`PACKED`): `data` is a string containing a [listpack](#listpack-encoding), each listpack entry is one list element.
    * Any other value is invalid.

The list is the concatenation of all nodes in order.

**Example:**

```
12 01 6C 01 02 0D 0D 00 00 00 02 00 81 61 02 81 62 02 FF
```

* `12`: value type `18`, `01 6C`: key `"l"`
* `01`: one node
* `02`: container `PACKED`
* `0D`: a 13 byte string follows, which is the listpack: total bytes `0D 00 00 00` = 13, `02 00` = 2 entries, `81 61 02` = `"a"`, `81 62 02` = `"b"`, `FF` = end
* The list is `["a", "b"]`

Source: `rdbLoadObject` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L2715-L2790), container values in [`src/quicklist.h` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/quicklist.h#L148-L149).

### Hash with Field TTLs

Redis 7.4 (RDB 12) added expiration of individual hash fields (`HEXPIRE`). Hashes where at least one field has a TTL
are stored with one of four types; hashes without field TTLs keep using types `4` and `16`. All TTLs are absolute Unix
timestamps in milliseconds when expanded; the maximum valid value is `2^48 - 1`.

`22` (`HASH_METADATA_PRE_GA`) and `23` (`HASH_LISTPACK_EX_PRE_GA`) were written by the 7.4 release candidates. Redis
7.4 GA and later write `24` and `25`, but still load all four.

#### Hash metadata (types 24 and 22)

Hashtable encoding:

```
[min-expire]                # type 24 only: 8 byte little endian ms timestamp
<len>                       # number of fields, length-encoded
<ttl><field><value>         # repeated len times
```

* `min-expire`: the smallest field expiry of the hash, in ms.
* `ttl`: [length-encoded](#length-encoding), frequently in the 9 byte `0x81` form for type 22.
    * `0` means the field has no TTL.
    * Type 24: otherwise the TTL is **relative**: `expire_at = ttl + min-expire - 1`.
    * Type 22: otherwise the TTL is the **absolute** expiry in ms.
* `field`, `value`: [strings](#string-encoding).

**Example:**

```
18 01 68 00 68 E5 CF 8B 01 00 00 01 01 01 66 01 76
```

* `18`: value type `24`, `01 68`: key `"h"`
* `00 68 E5 CF 8B 01 00 00`: min-expire = `0x0000018BCFE56800` = 1700000000000
* `01`: one field
* `01`: ttl 1, so the field expires at `1 + 1700000000000 - 1` = 1700000000000
* `01 66`: field `"f"`, `01 76`: value `"v"`

#### Hash listpack with TTLs (types 25 and 23)

```
[min-expire]                # type 25 only: 8 byte little endian ms timestamp, a hint (may be 0)
<listpack>                  # string containing a listpack
```

The [listpack](#listpack-encoding) contains **triplets** `[field1, value1, ttl1, field2, value2, ttl2, ...]`, where each
`ttl` is a listpack integer holding the **absolute** expiry in ms, or `0` for no TTL (for both types 23 and 25).
Redis keeps the triplets ordered by TTL, with fields without TTL last.

Neither layout filters expired fields: a field whose TTL already passed is stored like any other field.

*rdb-rs* decodes types 22 to 25 as regular hashes (fields and values) and drops the field TTLs.

Source: `rdbLoadObject` (`RDB_TYPE_HASH_METADATA*` and `RDB_TYPE_HASH_LISTPACK_EX*` cases) and `rdbSaveObject` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L2545-L2711).

### Stream Encoding

Streams were introduced in RDB version 9 (Redis 5.0). Four types exist, each extending the previous one:

| Type | Name | Since RDB | Written by | Adds |
| ---- | ---- | --------- | ---------- | ---- |
| `15` | STREAM_LISTPACKS   | 9  | Redis 5.0 - 6.2 | base format |
| `19` | STREAM_LISTPACKS_2 | 10 | Redis 7.0       | first id, max deleted id, entries added; per group entries read |
| `21` | STREAM_LISTPACKS_3 | 11 | Redis 7.2 - 8.4 | per consumer active time |
| `26` | STREAM_LISTPACKS_4 | 13 | Redis 8.6       | IDMP (idempotent producer) section |

A stream ID is two 64 bit unsigned integers, `ms` and `seq`. Where IDs are stored as length-encoded integers, the
millisecond part of a real-world ID needs the 9 byte `0x81` [length encoding](#length-encoding). "Raw" IDs below are
16 bytes: `ms` and `seq`, each 8 bytes big endian.

```
<num-listpacks>                     # length-encoded
  <master-id>                       # string, exactly 16 bytes: raw ID
  <listpack>                        # string containing a listpack (non-empty)
  ...                               # repeated num-listpacks times
<length>                            # number of entries, length-encoded
<last-id.ms> <last-id.seq>          # length-encoded
[type >= 19]
  <first-id.ms> <first-id.seq>      # length-encoded
  <max-deleted-id.ms> <max-deleted-id.seq>  # length-encoded
  <entries-added>                   # length-encoded
<num-groups>                        # length-encoded
  <group-name>                      # string
  <last-id.ms> <last-id.seq>        # length-encoded
  [type >= 19] <entries-read>       # length-encoded
  <pel-size>                        # length-encoded, global pending entries list
    <id>                            # 16 bytes raw ID
    <delivery-time>                 # 8 bytes little endian ms
    <delivery-count>                # length-encoded
    ...                             # repeated pel-size times
  <num-consumers>                   # length-encoded
    <consumer-name>                 # string
    <seen-time>                     # 8 bytes little endian ms
    [type >= 21] <active-time>      # 8 bytes little endian ms
    <consumer-pel-size>             # length-encoded
      <id>                          # 16 bytes raw ID, must also be in the group PEL
      ...                           # repeated consumer-pel-size times
    ...                             # repeated num-consumers times
  ...                               # repeated num-groups times
[type >= 26]
  <idmp-duration>                   # length-encoded, seconds (1 - 86400)
  <idmp-max-entries>                # length-encoded (1 - 10000)
  <num-producers>                   # length-encoded
    <producer-id>                   # string
    <num-entries>                   # length-encoded
      <iid>                         # string, idempotency id
      <id.ms> <id.seq>              # length-encoded, stream ID created for it
      ...                           # repeated num-entries times
    ...                             # repeated num-producers times
  <iids-added>                      # length-encoded
  <iids-duplicates>                 # length-encoded
```

For older types, the fields that are not stored are computed by Redis while loading. A type 26 stream always contains
the IDMP section, even when no producer uses it (`num-producers` is then `0`, followed by the two counters).

Each listpack holds the entries whose IDs follow its master ID. It starts with a master entry:

```
<count> <deleted> <num-fields> <field-1> ... <field-N> <0>
```

followed by one record per entry:

```
<flags> <ms-diff> <seq-diff> <num-fields> <field-1> <value-1> ... <field-N> <value-N> <lp-count>
<flags> <ms-diff> <seq-diff> <value-1> ... <value-N> <lp-count>           # SAMEFIELDS flag set
```

Every item above is one listpack entry. `ms-diff` and `seq-diff` are relative to the master ID. `flags` bit 0 marks a
deleted entry and bit 1 (`SAMEFIELDS`) an entry with the same fields as the master entry. `lp-count` is the number of
listpack entries in the record, excluding itself, which allows walking backwards.

*rdb-rs* walks all four stream types to skip them; it does not report streams.

Source: `rdbLoadObject` (stream case) and `rdbLoadStreamIdmpEntries` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L3065-L3441),
listpack record layout in `streamAppendItem` in [`src/t_stream.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/t_stream.c#L580-L750).

### Module Encoding

Module data types were introduced in RDB version 8 (Redis 4.0).

#### Module values (type 7)

```
<module-id>                 # length-encoded, 64 bit
<module-value-stream>       # (opcode, payload)* terminated by opcode 0
```

The content is written by the module, but through an API that tags every value with an opcode, so the value can be
skipped without the module. *rdb-rs* skips type 7 values this way.

#### Module Id

The 64 bit module id identifies the module type and its encoding version:

* bits 63..10: the 9 character type name, 6 bits per character, first character in the most significant bits, using
  the charset `A-Z a-z 0-9 - _` (index 0 = `A`, 62 = `-`, 63 = `_`)
* bits 9..0: the encoding version of the module data

#### Module Value Stream

Used by module values (type 7), [module auxiliary data](#module-auxiliary-data) and [key metadata](#key-metadata).
It is a sequence of `<opcode><payload>` pairs, the opcode being [length-encoded](#length-encoding):

| Opcode | Name   | Payload |
| ------ | ------ | ------- |
| `0`    | EOF    | none, ends the stream |
| `1`    | SINT   | length-encoded integer, the signed 64 bit value as unsigned (negative values use the `0x81` form) |
| `2`    | UINT   | length-encoded integer |
| `3`    | FLOAT  | 4 bytes, little endian IEEE 754 float |
| `4`    | DOUBLE | 8 bytes, little endian IEEE 754 double |
| `5`    | STRING | [string](#string-encoding) (also used for long doubles, stored as text) |

Any other opcode is invalid.

#### Pre-release module values (type 6)

Type `6` (`MODULE_PRE_GA`, originally `MODULE`) was written by the Redis 4.0 release candidates. Its content has no
opcodes and no end marker, so it can only be parsed by the module. Redis itself refuses to load it, and *rdb-rs* stops
with an "unsupported" error.

Source: `rdbLoadObject` (`RDB_TYPE_MODULE_2` case) and `rdbLoadCheckModuleValue` in [`src/rdb.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.c#L1926-L1973), sub opcodes in [`src/rdb.h` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/rdb.h#L102-L108), module id in `moduleTypeNameByID` in [`src/module.c` at 8.6.7](https://github.com/redis/redis/blob/8.6.7/src/module.c#L7263).

## CRC64 Checksum

Starting with RDB Version 5, an 8 byte CRC64 checksum is added to the end of the file. It is possible to disable this checksum via a parameter in redis.conf.
When the checksum is disabled, this field will have zeroes.
