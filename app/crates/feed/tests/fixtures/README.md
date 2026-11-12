# Feed Fixtures

Throwaway minisign keys and signatures in the form Tauri's `signer sign` writes: each
`.pub` and `.sig` file is the base64 of the minisign text, and each signature is the
prehashed `ED` form over `release-file.bin`, with a trusted comment that names the
version as `version:<v>` (`unversioned.sig` names none). `other-key.pub` is a second key
that signed nothing. No release key is here; the private keys were not kept.

They were made once with Python's `cryptography` package (Ed25519) and `hashlib`'s
BLAKE2b-512, following the minisign format.

`.gitattributes` keeps these files byte for byte, since a checkout that changed line
endings would change the signed bytes.
