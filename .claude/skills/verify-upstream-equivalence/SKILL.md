---
name: verify-upstream-equivalence
description: Check that the Rust message parser produces the same AST as @rocket.chat/message-parser 0.33.0, by running both on every short markup string and on generated token sequences, under every option. Use before committing any change under src/parser/, src/tld.rs, src/tld/ or src/ast.rs, and when an AST looks different from what Rocket.Chat renders.
metadata:
  covers:
    - src/ast.rs
    - src/lib.rs
    - src/parser/
    - src/tld.rs
    - src/tld/
  check: 'upstream=$(mktemp -d) && cp upstream/package.json upstream/bun.lock "$upstream" && (cd "$upstream" && bun install --frozen-lockfile) && UPSTREAM_PARSER_DIR="$upstream" TZ=UTC cargo test --release --features upstream-oracle --test differential -- --ignored'
---

# Verify upstream equivalence

**Property.** For every input the check generates and each of nine option sets, `parse_at(input, options, clock)` serialized to JSON equals what `parse(input, options)` from `@rocket.chat/message-parser` 0.33.0 returns on Node at the same clock and timezone. Key order does not matter; numbers compare as JavaScript numbers. If upstream throws, or Rust panics, that counts as a mismatch.

**Harm if broken.** A message renders differently in clients that use this crate than in Rocket.Chat: bold becomes literal asterisks, a link is lost or points somewhere else, a mention or emoji is missing.

## Run

Install the pinned upstream package once. Its `node_modules` is gitignored:

```sh
(cd upstream && bun install --frozen-lockfile)
TZ=UTC cargo test --release --features upstream-oracle --test differential -- --ignored --nocapture
DIFFERENTIAL_TOKEN_LENGTH=3 TZ=UTC cargo test --release --features upstream-oracle --test differential -- --ignored --nocapture
```

The first line is the fast tier, which `metadata.check` runs before every commit: token sequences of up to 2 pieces, about 20 seconds. The second is the full tier, sequences of up to 3 pieces, about 2 minutes. Run the full tier, under `TZ=UTC` and `TZ=America/New_York`, before every push; nothing else runs it. Each test splits its (input, option set) pairs across one oracle process per core; set `DIFFERENTIAL_JOBS` to change the count. Both need `node` on `PATH`. To keep `node_modules` out of the checkout, copy `upstream/package.json` and `bun.lock` into a `mktemp -d` directory, run `bun install --frozen-lockfile` there, and set `UPSTREAM_PARSER_DIR` to that directory. The oracle refuses to run on any version other than 0.33.0.

`tests/differential.rs` also holds four tests that are not ignored and that CI runs under `TZ=America/New_York`: `upstream_test_suite_inputs_produce_the_same_ast` (the inputs in `tests/fixtures/upstream/*.json`, with each case's options and clock), `regression_inputs_produce_the_same_ast` (`tests/fixtures/differential/regressions.json`), `every_emoji_produces_the_same_ast`, and the proptest `generated_messages_produce_the_same_ast` (random options and clock; `PROPTEST_CASES` sets the count). `-- --ignored` skips them; this skill's tiers are the three tests below.

The three tests below in `tests/differential.rs` are `#[ignore]`d, so `cargo test` and the CI run of that file skip them. Each one prints `N inputs x 9 option sets: every AST matches upstream` when it passes:

- `every_short_character_string_matches_upstream`: every string of 1 to 3 characters over `WIDE_ALPHABET` (19 markup characters, a letter, a digit, space and newline), plus every 4-character string over `CORE_ALPHABET` (13). 41,280 inputs.
- `every_short_token_sequence_matches_upstream`: every sequence of 1 to `DIFFERENTIAL_TOKEN_LENGTH` (default 2, full tier 3) entries of `TOKENS` (80 markup pieces: `<t:…:R>`, `- [ ] `, `http://a.b`, `a@b.co`, `color:#f00`, `:smile:`, `\(`, `😀`, block spoiler and long code fence lines (including a spoiler opener followed by CRLF), timestamps outside the int32 seconds range, invalid dates and time-of-day timestamps, spoiler starts `||:)`, `||-___-` and `||*a*`, custom-domain hosts `x.local`, `X.LOCAL` and `http://x.Local/a`, `http://localhost`, `@.:]` (which makes the hostname `.`), URL schemes (including dotted 32- and 33-character prefixes before `://`), a lone `\r` and so on). 6,453 distinct inputs in the fast tier, 514,139 in the full tier. With planted bugs, the fast tier still failed on an empty custom domain linking (7 pairs here, 80 in the random test), int32 timestamp wrap dropped (3,888 and 24,407) a CR after a block spoiler opener rejected (72 in the random test), and `has_absolute_scheme_prefix`'s cap raised to 32 or lowered to 30 characters (978 and 980 pairs here).
- `random_token_sequences_match_upstream`: `DIFFERENTIAL_CASES` (default 20,000) sequences of 1 to 12 pieces, each a `TOKENS` entry or a `WIDE_ALPHABET` character, from the seed `DIFFERENTIAL_SEED` (default `0x5eed0033`). Raise the count and change the seed for a deeper run.

The option sets are in `option_sets()`: default (`{}`), explicit defaults (every key present and off: `false`, `katex` with both flags `false`, `customDomains: []`), `emoticons`, `colors`, `katex.dollarSyntax`, `katex.parenthesisSyntax`, `customDomains: ["a", "local", ".", ""]`, `customDomains: ["LOCAL", "ocal", "chat", "", "."]` (near misses: wrong case, a suffix without a dot boundary, a real TLD, the empty string), and all of the first seven together. Both `customDomains` lists hold `.` and `""`, the only hosts that match the hostname `.`, in opposite orders: the first matching host decides, so `.` first links and `""` first does not. Each set is written once, as upstream's options object: the oracle passes it to upstream as is, and Rust gets `Options` by deserializing the same JSON (through `JsOptions`), so the check also covers the `Deserialize` mapping from upstream's names. The clock is `CLOCK_SECONDS` (2025-07-22T00:00:00Z) on both sides: Rust gets it through `parse_at`, and `upstream/oracle.mjs` replaces Node's `Date` so `new Date()` and `Date.now()` return the `now` each request carries. Both processes share the `TZ` you set. Run once more with `TZ=America/New_York` when a change touches `src/parser/timestamp.rs`, because times without an offset resolve in the local zone.

## Reading a failure

The panic message gives the number of failing (input, option set) pairs. It then minimizes up to 400 of them by deleting characters for as long as the input still mismatches, and prints each distinct result with its option set, upstream's JSON and Rust's JSON. Start from the shortest one.

- **Real bug.** The Rust AST differs and the README's "Differences from upstream" section does not list the case. Find the upstream rule in the grammar (the oracle parses with the same `dist/messageParser.js` that `upstream/` installs), then the matching rule in `src/parser/`. When you fix it, add the minimized input to `tests/fixtures/differential/regressions.json` as `{ "input": …, "options": …, "now": 1753142400000 }`, with `options` copied from the named set in `option_sets()`, or add a focused test in `tests/`. `tests/fixtures/upstream/*.json` holds only upstream's own test suite.
- **Documented difference.** Empty input (upstream throws, Rust returns `[]`), lists deeper than 64 levels, lone surrogates (Rust `&str` cannot hold one, so the generator never makes them), and the `work-report` feature. The generators never produce the first three. If you deliberately add another difference, write it in the README first, then keep the generators from producing it instead of filtering results.
- **Harness error.** `upstream: upstream threw: …` on a non-empty input is a real mismatch, not a harness error: upstream's grammar can fail where Rust returns an AST. A harness error looks like `oracle exited early`, a JSON parse panic, or the install message; fix the setup, not the property.
- **Clock or zone.** A mismatch only in a `<t:…>` value that moves with `TZ` or with the clock means one side read the real clock or zone. Check that `ParseCase::rust` passes the case's `now` to `parse_at` and that the oracle still installs `PinnedDate` before it imports the parser.

Do not weaken the comparison, drop an option set, or shrink an alphabet to make a run pass. A mismatch stays a finding until the code or the README changes.

## Coverage

| Check | Code it covers |
| --- | --- |
| AST shape and serialization | `Node`, the untagged `Emoji` enum and their serde attributes in `src/ast.rs` |
| Every rule reachable from the alphabets and tokens | `src/parser/{mod,blocks,inline,links,emoji,timestamp,reduce,repetition,memo,chars,char_tables}.rs` |
| Option handling | `Options`, `KatexOptions` and their `Deserialize` attributes (camelCase names, `default`) in `src/lib.rs`; every `self.options` read in `src/parser/` |
| Autolinks and TLDs | `auto_link`, `resolve_href` in `src/parser/links.rs`; `is_linkable` in `src/tld.rs` for the hosts in `TOKENS` (`b.com`, `a.b`, `rocket.chat`, `b.co`, `c.co`, `x.local`, `X.LOCAL`, `x.Local`, `localhost`, and `.` through `@.:]`) and both `customDomains` sets, including which of `""` and `.` comes first; `link_hosts` in `src/parser/links.rs`. `tests/custom_domains.rs` also pins `b.m@.:]` |
| Scheme prefix when `url::Url::parse` fails | `has_absolute_scheme_prefix` in `src/parser/links.rs`, through `http://a.b:99999`, `:99999` and the 32- and 33-character schemes in `TOKENS`. The dotted prefixes `abcdefghijklmnopqrstuvwxyz01.com://` and `abcdefghijklmnopqrstuvwxyz012.com://` pin its 32-character cap: the grammar's scheme rule takes no dots, so they link as bare hosts and reach the check |
| Block closers | `block_spoiler`, `spoiler_paragraph_step`, `long_code`, `next_long_code_close`, `fence_close`, `long_code_closes` in `src/parser/blocks.rs`, and the CR, LF and CRLF line breaks they take from `end_of_line` in `src/parser/mod.rs` and `line_break_length` in `src/parser/chars.rs`, through `"\|\|\n"`, `"\|\|\r\n"`, `"\n\|\|"`, `"\r"`, `"a\nb"`, `` "````\n" ``, `` "\n````" `` and `` "`````" `` in `TOKENS` |
| Emoticons and shortcodes | `src/parser/emoji.rs`, `join_emoji` in `src/parser/reduce.rs` |
| Inline spoilers | `spoiler`, `spoiler_content_item` and `inline_emoticon` lookahead in `src/parser/inline.rs`, `Repetition::Spoiler` in `src/parser/repetition.rs`, through `"\|\|:)"`, `"\|\|-___-"` and `"\|\|*a*"` in `TOKENS` (spoilers holding an emoticon or emphasis, followed by a later `\|\|`) |
| Relative and absolute timestamps | `src/parser/timestamp.rs`, at one clock and the zone in `TZ`; `epoch_seconds` wrapping through the 2038, year 9999 and year 1800 timestamps in `TOKENS`, and its `"0"` fallback through `<t:2025-13-45T25:61:61>`, `+24:00` offsets and `<t:12:00>` |

`.claude/skills/verify-linear-parse` covers work and stack depth; this skill covers only output.

**Sync commit:** `42daa27`

When covered code changes, compare it against the sync commit (`git diff 42daa27 -- src tests/common`). For each new rule, option, marker character or token, check that `WIDE_ALPHABET`, `CORE_ALPHABET` or `TOKENS` can produce it, and add it if not. Run the check, then move the sync commit to the commit you confirmed.

## What this does not prove

- It is bounded, not total. Exhaustive coverage ends at 3 characters over `WIDE_ALPHABET`, 4 over `CORE_ALPHABET`, and 2 pieces over `TOKENS` (3 in the full tier). Longer inputs are sampled at random.
- Characters outside the alphabets and tokens are not exercised: other Unicode emoji and their modifiers, most of the TLD list, tabs, and non-ASCII letters in links and mentions. `tests/emoji.rs` covers the emoji table and `src/tld/tests.rs` covers host linkability against upstream data.
- One clock and one zone per run. Daylight saving edges depend on `TZ`; `tests/local_time.rs` covers them.
- Lists nested past 64 levels and inputs over a few hundred characters are never generated.
- Local times without an offset before 1883 are not generated: chrono reads the OS zone data and Node reads ICU, and their local mean time offsets may differ.
- Memo reuse that does not change output is not checked here: the `Run::SpoilerParagraphs` memo is only hit again on a run whose spoiler already failed at the same close. Breaking it leaves every AST the same; `.claude/skills/verify-linear-parse` covers it.
- The emoticon lookahead cap (`MAXIMUM_EMOTICON_LOOKAHEAD_DEPTH`, 500 levels) is never reached: the generated inputs are a few hundred characters at most. Past the cap the output differs from upstream by design, as the README says. A one-off comparison found `||:)` matching upstream up to 501 repetitions.
- Options objects outside upstream's TypeScript type are never sent: `null` values, unknown keys (Rust rejects them, as the README says; upstream ignores them), and non-boolean flags.
