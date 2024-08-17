# opusmeta

opusmeta is a Rust crate for reading and writing metadata from opus files, created for the [multitag project](https://crates.io/crates/multitag).

See the `read_tags` example for basic usage. To run it, type:
```sh
cargo run --example read_tags [input_file.opus]
```
### Tag names
Unlike the more structured ID3 format, the Opus spec does not mandate a common set of tag names or values. However, a list of common tag names can be found [here](https://xiph.org/vorbis/doc/v-comment.html).
