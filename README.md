# rocket.chat-message-parser-rs

Rust implementation of the Rocket.Chat message-parser grammar.

It produces the same AST as `@rocket.chat/message-parser` 0.33.0, and parsing time grows linearly with message length.

```rust
use message_parser::{parse, Options};

let nodes = parse("**hello** :smile:", &Options::default());
```

`parse_at` takes the current time explicitly. Relative timestamps such as `<t:10:00+01:00>` resolve against it.

## Differences from upstream

- Empty input returns an empty list. Upstream throws.
- Lists nest at most 64 levels deep. Deeper lines become items of the 64th level, so no text is lost. Upstream nests without limit, and a tree that deep can overflow the stack when it is serialized or dropped.

## Work report

The `work-report` feature adds `parse_with_report`, which returns the AST with a `ParseReport` that counts input reads and rule calls. The linear-time test and the benchmark use it:

```sh
cargo test --release --features work-report --test linear_work
cargo run --release --features work-report --example benchmark
```

## Comparing with upstream

`upstream/` installs `@rocket.chat/message-parser` 0.33.0. Install it once before either comparison:

```sh
(cd upstream && bun install)
```

### Same AST

The `differential` test starts `upstream/oracle.mjs` on Node and parses the same input with both parsers, pinning upstream's clock to the one Rust gets. It fails on any difference in the AST. It compares:

- every input of the upstream test suite (`tests/fixtures/upstream`), with the options and clock each test uses,
- every emoji in `tests/fixtures/emoji/all-emoji.json`, alone and between words,
- 20,000 generated messages: random mixes of markup characters, links, mentions, emoji, emoticons, KaTeX, colors and timestamps, with every combination of parse options.

```sh
TZ=America/New_York cargo test --release --features upstream-oracle --test differential
```

`PROPTEST_CASES` changes the number of generated messages. A failing message is shrunk to a minimal input. Add that input to `REGRESSIONS` in `tests/differential.rs` with the fix, so every later run checks it whatever the generator produces. CI runs the test on every push.

### Speed

`upstream/compare.mjs` parses the inputs in `benchmarks/scenarios.json` with this crate and with upstream, and prints the median of each side by side:

```sh
node upstream/compare.mjs
```
