# courtside

A command-line tool for volleyball scouting files: check DataVolley (`.dvw`) and VolleyStation (`.vsm`) files for problems, convert `.dvw` matches to JSON for R, Python or spreadsheets, and print match statistics.

Free software under the [GNU AGPL v3](LICENSE). This repository holds the CLI's source and its release binaries.

## Install

**macOS (Homebrew)**

```sh
brew install cillianbc/tap/courtside
```

**macOS / Linux**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/cillianbc/courtside-cli/releases/latest/download/courtside-cli-installer.sh | sh
```

**Windows (PowerShell)**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/cillianbc/courtside-cli/releases/latest/download/courtside-cli-installer.ps1 | iex"
```

Or download an archive for your platform from [Releases](https://github.com/cillianbc/courtside-cli/releases).

## Use

```sh
courtside validate match.dvw          # list problems; exit code 1 if any errors
courtside convert match.dvw match.json
courtside stats match.dvw             # set scores, team and player skill counts
```

`validate` checks that a file is readable and internally consistent (required sections, team and roster entries, the match date, unrecognised scouting codes, a lossless read and write), then the match itself: score, rotation, substitutions, liberos and back-row play, and how each rally's touches fit together.

`convert` writes one JSON record per scouting line, with field names matching the `plays` columns of the [openvolley](https://github.com/openvolley/datavolley) R package.

## Build from source

With a Rust toolchain (1.85 or later):

```sh
cargo install --path crates/courtside-cli
```

`crates/` holds the `.dvw`/`.vsm` reader and writer (`dvw-codec`), the scouting-code grammar (`vb-grammar`), the match engine and validator (`vb-engine`), the JSON views (`courtside-ffi`) and the CLI. Tests that need real match files are skipped without them.

## Feedback

Found a file that doesn't read correctly? [Open an issue](https://github.com/cillianbc/courtside-cli/issues). If you can share the file, it helps us support it.
