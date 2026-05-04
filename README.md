# rupo

A blazing-fast alternative to [repo](https://gerrit.googlesource.com/git-repo/), written in Rust.

> ⚠️ This project is in early development. Not ready for production use.

## Features

- **Set-based group filtering** — order-independent matching, an improvement over repo (`-g default,-vendor`)
- **Cross-platform** — builds on Linux, macOS, and Windows

## Quick Start

```sh
# Initialize workspace from a manifest repository
rupo init -u <manifest-url> [-b <branch>] [-m <manifest.xml>] [-g <groups>] [--depth <N>]

# Sync all projects
rupo sync [-j <jobs>] [-g <groups>]
```

## Commands

### `rupo init`

Initialize a rupo workspace. Clones the manifest repository, parses the
XML manifest, and writes the internal config/manifest files under `.rupo/`.

| Option | Description |
|---|---|
| `-u, --url <URL>` | Manifest repository URL (required) |
| `-b, --branch <REV>` | Branch or revision to use |
| `-m, --manifest <FILE>` | Manifest filename (default: `default.xml`) |
| `-g, --groups <GROUPS>` | Restrict projects to group(s): `default`, `all`, `G1,G2,-G3` |
| `--depth <N>` | Shallow clone depth for project syncs |

### `rupo sync`

Sync all projects in the workspace. Projects are cloned on first run and
fetched on subsequent runs. Linkfiles and copyfiles are applied after sync.

| Option | Description |
|---|---|
| `-j, --jobs <N>` | Number of parallel jobs (default: CPU core count) |
| `-c, --current-branch` | Only sync the current branch |
| `-g, --groups <GROUPS>` | Override group filter for this sync only |

## Manifest Compatibility

rupo reads the same XML manifest format as repo. Supported elements:

| Element | Status |
|---|---|
| `<remote>` | ✅ Supported |
| `<default>` | ✅ Supported (`revision`, `remote`) |
| `<project>` | ✅ Supported (`name`, `path`, `revision`, `remote`, `groups`) |
| `<linkfile>` | ✅ Supported (`src`, `dest`) |
| `<copyfile>` | ✅ Supported (`src`, `dest`) |
| `<include>` | ❌ Not yet |
| `<extend-project>` | ❌ Not yet |
| `<remove-project>` | ❌ Not yet |

## Group Filtering

rupo uses **set-based matching** (an improvement over repo's order-dependent algorithm):

- Every project implicitly belongs to `all` and `default`
- Projects with `notdefault` in their groups are excluded from `default`
- `-g default,-vendor` means: include default group, exclude vendor group
- Order does not matter — `default,-vendor` and `-vendor,default` are equivalent

## Building

```sh
cargo build --release
```

## License

[Apache-2.0](LICENSE)
