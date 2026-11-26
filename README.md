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
- With emoticons on, an emoticon looks ahead at most 500 levels deep into spoilers that each end in another emoticon, as in `||:)||:)||:)…`. Past that depth the lookahead counts as not followed, so one emoticon in every 500 parses as plain text. This keeps the stack under about 1.3 MiB in a release build, within a 2 MiB thread. Upstream recurses without limit and overflows Node's default stack at about 1,500 levels.
- `Options` is strict. `null`, a number or a string where upstream expects a boolean, object or list is an error, as is an unknown key. Upstream reads `null` as off, any truthy value as on, and ignores unknown keys.
- Times without an offset, such as `<t:2025-07-22T10:00:00>`, resolve in the local zone using the operating system's time zone database. Upstream on Node uses the ICU data bundled with Node. The two can differ for an invalid or POSIX-style `TZ` value, and for zones whose rules changed between the two databases' versions.

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

`PROPTEST_CASES` changes the number of generated messages. A failing message is shrunk to a minimal input. Each failure starts with that case as one line of JSON: its input, options and clock. Add the line to `tests/fixtures/differential/regressions.json` with the fix, so every later run checks that case whatever the generator produces. Copy that JSON line, not proptest's `minimal failing input` block. CI runs the test on every pull request and every push to `main`.

The same file also checks every short markup string and token sequence under nine fixed option sets. Those tests are `#[ignore]`d because they take minutes; `.claude/skills/verify-upstream-equivalence` says how to run them.

### Speed

`upstream/compare.mjs` parses the inputs in `benchmarks/scenarios.json` with this crate and with upstream, and prints the median of each side by side:

```sh
node upstream/compare.mjs
```
