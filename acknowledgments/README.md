This directory contains pre-generated acknowledgments for the Rust dependencies of RingRTC. CI enforces that they are kept up to date.

## Updating

If you update RingRTC's dependencies, you'll need to update this listing. Install [cargo-about][] if you haven't already:

```shell
cargo +stable install --locked cargo-about --version 0.6.0
```

Then:

1. Run `bin/regenerate_acknowledgments.sh`.
2. Check the HTML output for new "synthesized" entries. This can indicate that the license for a particular dependency was not properly detected.
3. If there are any "synthesized" entries (besides those in this repository, "ringrtc" and "regex-aot"), add new "[clarify][]" entries to about.toml.

[cargo-about]: https://embarkstudios.github.io/cargo-about/
[clarify]: https://embarkstudios.github.io/cargo-about/cli/generate/config.html#the-clarify-field-optional
