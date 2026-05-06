# hdiffpatch-rs

Rust library for applying HDiffPatch-style patches, with a small compatibility layer for the `KrDiff` format.

<!-- TOC -->
* [hdiffpatch-rs](#hdiffpatch-rs)
  * [Format support](#format-support)
  * [Compression modes](#compression-modes)
  * [Installing](#installing)
  * [Usage](#usage)
  * [Credits](#credits)
  * [Contributing](#contributing)
<!-- TOC -->

## Format support

| Format      | Status    | Notes                                                                                       |
|-------------|-----------|---------------------------------------------------------------------------------------------|
| `HDIFF13`   | Supported | Single-file patching                                                                        |
| `HDIFF19`   | Supported | Directory patching                                                                          |
| `HDIFFSF20` | Supported | Single-compressed patching                                                                  |
| `KrDiff`    | Supported | Modified `HDIFF19`-style directory patch format from KuroGames made for WutheringWaves game |

## Compression modes

| Compression      | Status          |
|------------------|-----------------|
| `zstd`           | Supported       |
| `zlib`           | Not implemented |
| `bzip2`          | Not implemented |
| `lzma` / `lzma2` | Not implemented |

## Installing

```toml
[dependencies]
hdiffpatch-rs = { git = "https://github.com/TwintailTeam/hdiffpatch-rs", branch = "master" }
```
## Usage

```rust
use hdiffpatch_rs::patchers::HDiff;
use hdiffpatch_rs::patchers::KrDiff;

// Pass directory for source path and output path if it is a directory diff (HDIFF19)
fn main() {
    let source_path = String::from("./old_file.bin");
    let patch_path = String::from("./update.hdiff");
    let output_path = String::from("./new_file.bin");

    let mut patcher = HDiff::new(source_path, patch_path, output_path);

    if patcher.apply() {
        println!("Patch applied successfully");
    } else {
        eprintln!("Patch failed");
    }
}

// KrDiff sample
fn main() {
    let source_dir = String::from("./game");
    let patch_path = String::from("./update.krpdiff"); // .krdiff also works!
    let output_dir = String::from("./patched");

    let mut patcher = KrDiff::new(source_dir, patch_path, output_dir);

    if patcher.apply() {
        println!("KrDiff patch applied successfully");
    } else {
        eprintln!("KrDiff patch failed");
    }
}
```

## Credits

This project exists because of the original [HDiffPatch](https://github.com/sisong/HDiffPatch) project by sisong. `hdiffpatch-rs` is a Rust implementation patch applier for compatible formats, credit for the original HDiffPatch format and tooling belongs upstream.

Additional credit goes to CollapseLauncher's [SharpHDiffPatch.Core](https://github.com/CollapseLauncher/SharpHDiffPatch.Core), a C# HDiffPatch port whose structure helped guide parts of this crate's parser and patcher logic.

## Contributing

Contributions and issues are welcome this port is not perfect and for sure will have some issues people can find! if you do find them do not be scared to open an issue or a pull request with a fix!
