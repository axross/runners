# IBM Plex Sans

The three files here are IBM Plex Sans, bundled so that the windows download nothing
at run time. The pages use these weights only: Regular (400), Medium (500) and
SemiBold (600), in WOFF2, which WebView2 reads. `../fonts.css` is the one place that
names them.

- **Release:** `@ibm/plex-sans` 1.1.0, published 2024-11-11 by IBM.
- **Source:** <https://github.com/IBM/plex/releases/download/%40ibm%2Fplex-sans%401.1.0/ibm-plex-sans.zip>, the release's `ibm-plex-sans.zip`.
- **Archive SHA-256:** `fb365d910566e6d199cc2c15579a7dd9a267128e18431a394ed81f1970c69200`.
- **Taken from the archive:** `ibm-plex-sans/fonts/complete/woff2/` for the three fonts, and `ibm-plex-sans/LICENSE.txt` for the licence. The files are byte for byte what the archive holds.
- **Licence:** SIL Open Font License 1.1, copyright 2017 IBM Corp. with Reserved Font Name "Plex". `LICENSE.txt` is its text, which ships with the app. The repository's `.gitattributes` keeps these files byte for byte, line endings included.

| File                         | SHA-256                                                            |
| ---------------------------- | ------------------------------------------------------------------ |
| `IBMPlexSans-Regular.woff2`  | `ba711a3085ff9f27440b6b9c4550cfc47c97bf36591d5da958b975bb3add8c1a` |
| `IBMPlexSans-Medium.woff2`   | `5660f8a658f8bb50dbc005232f885eadffd2bc1c235c4f6fbb63469d1f9cde6d` |
| `IBMPlexSans-SemiBold.woff2` | `f78048030eab62e860efa39a0df79e2e5581bf122eb95b9bc42c0b8a4988d205` |
| `LICENSE.txt`                | `7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da` |

To check them, run `sha256sum` on each file and compare. To update, download the new release's archive, copy the same four files, and change the release, source and every hash above in one commit.
