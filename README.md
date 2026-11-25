# rocket.chat-message-parser-rs

Rust implementation of the Rocket.Chat message-parser grammar.

It produces the same AST as `@rocket.chat/message-parser` 0.33.0, and parsing time grows linearly with message length.

```rust
use message_parser::{parse, Options};

let nodes = parse("**hello** :smile:", &Options::default());
```

`Options` deserializes from upstream's options object (`colors`, `emoticons`, `katex.dollarSyntax`, `katex.parenthesisSyntax`, `customDomains`), and rejects unknown keys.

`parse_at` takes the current time explicitly. Relative timestamps such as `<t:10:00+01:00>` resolve against it.

## Differences from upstream

- Empty input returns an empty list. Upstream throws.
- Lists nest at most 64 levels deep. Deeper lines become items of the 64th level, so no text is lost. Upstream nests without limit, and a tree that deep can overflow the stack when it is serialized or dropped.
- Input cannot contain lone surrogates, because a Rust `&str` cannot hold them. A caller converting a JavaScript string must replace them, for example with U+FFFD, before parsing; upstream keeps them.
- Input of `u32::MAX` UTF-16 code units or more panics. Positions are stored as `u32`.

## Work report

The `work-report` feature adds `parse_with_report`, which returns the AST with a `ParseReport` that counts input reads and rule calls. The linear-time test and the benchmark use it:

```sh
cargo test --release --features work-report --test linear_work
cargo run --release --features work-report --example benchmark
```

## Comparing with upstream

`benchmarks/upstream/compare.mjs` parses the inputs in `benchmarks/scenarios.json` with this crate and with `@rocket.chat/message-parser` 0.33.0 on Node, and prints the median of each side by side:

```sh
cd benchmarks/upstream
bun install
node compare.mjs
```
