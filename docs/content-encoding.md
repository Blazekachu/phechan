# Content encoding / compression notes

Date: 2026-09-17  
Phase: 7

## Tag 9

Ordinals **content_encoding** (tag 9) is a **hint for indexers/clients**, not a proof that every viewer decompresses.

## Phechan policy

1. Default create path ships **uncompressed** bodies.  
2. Optional Brotli via UI checkbox or CLI `--compress-br` (also `--content-encoding br`): compresses the body and sets tag 9 to `br`.  
3. Recommended MIME types: `text/*` (including `text/html`), `application/json`, `application/javascript`, `application/xml`, `image/svg+xml`.  
4. Never claim “compressed inscriptions save fee” without measuring serialized envelope + witness size on the target network.  
5. Validation: if tag 9 present, surface note: `content_encoding set; clients may not decompress`.

## Experiment checklist

- [x] CLI `--compress-br` + envelope tag 9  
- [x] UI checkbox when MIME is compressible  
- [ ] Measure envelope size raw vs br for sample payloads on target `ord`  
- [ ] Verify `ord` version under test renders / refuses  
