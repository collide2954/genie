# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library.

genie is not complete. There is no release.

## Run

Install [rustup](https://rustup.rs). rustup installs the toolchain version in `rust-toolchain.toml`.

```bash
cargo run -p genie
```

## Test

```bash
cargo test --workspace
```

## Crates

| Crate | Contents |
| --- | --- |
| `genie-core` | Genomic coordinates, loci and the viewport |
| `genie-formats` | Parsers for genomic file formats |
| `genie-render` | Track layout and draw primitives |
| `genie` | The iced application |

## License

Apache-2.0. Refer to [LICENSE](LICENSE).
