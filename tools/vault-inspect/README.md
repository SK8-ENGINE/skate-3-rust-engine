# Vault inspection

Look up values in the game's attribute database ("vault"): the skater collections setup converts to
`assets/private/stock/skater-collections.json`, and the class layouts in the disc's schema.
Classes, collections and fields are 64-bit name hashes (`Hash_XXXXXXXXXXXXXXXX`).

| Script | What it does |
|---|---|
| `find_field.py` | Finds field hashes anywhere in the converted database: prints class, collection, field, type and the decoded value. |
| `vault_fields.py` | Prints one class's collections (or one collection), with parent, and every field (or the chosen ones), floats / ints decoded and arrays listed. |
| `vault_layout.py` | Prints a class's field layout from the schema: field hash, type, offset, count, maximum, flags, alignment. Fixed-layout fields (flags bit 2) first, by offset. |

## Inputs

- `--vault`: the converted database, default `assets/private/stock/skater-collections.json` (written by
  `tools/setup.py`).
- `--schema` (for `vault_layout.py`): the schema path without extension; the folder must hold
  `skaterschema.vlt` and `skaterschema.bin`. Extract them from your disc's `data/big/db.big`:
  `py -3.13 tools/world-stream-inspect/big_list.py <disc>/data/big/db.big skaterschema --out .local/db`

## Usage

```
py -3.13 tools/vault-inspect/find_field.py 0123456789ABCDEF
py -3.13 tools/vault-inspect/vault_fields.py Hash_0123456789ABCDEF default
py -3.13 tools/vault-inspect/vault_fields.py 0123456789ABCDEF default FEDCBA9876543210
py -3.13 tools/vault-inspect/vault_layout.py 0123456789ABCDEF --schema .local/db/data/db/skaterschema
```

## Example output

(made-up hashes and values)

```
== Hash_0123456789ABCDEF default parent=
  Hash_1111111111111111 ('EA::Reflection::Float', 0.5)
  Hash_2222222222222222 ('EA::Reflection::Int32', 3)
```

## Requirements

Python 3.13 (standard library); `vault_layout.py` uses `tools/asset_pipeline/vlt.py`.
