# Metadata, title & metaprotocol (protocol-accurate)

Reference: [Ordinals inscriptions](https://docs.ordinals.com/inscriptions.html), [Properties](https://docs.ordinals.com/inscriptions/properties.html)

## Title → Properties (tag 17)

- **Ordinals field:** Properties CBOR, Attributes key `0` (title)
- **Not** Metadata (tag 5). Explorers (e.g. ordinals.com) show this as **title**
- Phechan `--title` / UI Title → tag 17 only
- Example: Title `BHANG` with empty Metadata still shows title on-chain; no metadata row

## Metaprotocol (tag 7)

- **Type:** UTF-8 string (free-form identifier)
- **Allowed:** any printable string you choose (e.g. `bhang`) — there is **no registry** and no requirement that a named protocol “exists”
- **Semantics:** not universal; clients/indexers may ignore unknown values
- **Not:** MIME type, JSON blob, or Metadata

## Metadata (tag 5)

- **Type:** ideally CBOR; Phechan accepts UTF-8 text/JSON and pushes those bytes (chunked ≤520)
- **Independent of Title** — omit this field entirely if you only want a title
- **Not allowed:** private keys, seeds, WIFs

## Delegate (tag 11)

- Points at another inscription ID for content
- **No body** — content resolves from the target (may 404 until it exists)

## Content encoding (tag 9)

- Optional hint that the body is compressed (e.g. `br` for Brotli)
- See `docs/content-encoding.md`
