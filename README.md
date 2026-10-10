# courtside

A command-line tool for volleyball scouting files: check DataVolley (`.dvw`) and VolleyStation (`.vsm`) files for problems, and convert `.dvw` matches to JSON for R, Python or spreadsheets.

Free to use. This repository holds release binaries only.

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
```

`validate` checks that a file is readable and internally consistent: required sections, team and roster entries, the match date, unrecognised scouting codes, and that the file reads and writes back unchanged. Rule checks (score, rotation, substitutions) are coming in a later release.

`convert` writes one JSON record per scouting line, with field names matching the `plays` columns of the [openvolley](https://github.com/openvolley/datavolley) R package.

## Feedback

Found a file that doesn't read correctly? [Open an issue](https://github.com/cillianbc/courtside-cli/issues). If you can share the file, it helps us support it.
