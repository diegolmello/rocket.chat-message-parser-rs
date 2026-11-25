---
name: verify-upstream-equivalence
description: Check that the Rust message parser produces the same AST as @rocket.chat/message-parser 0.33.0, by running both on every short markup string and on generated token sequences, under every option. Use before committing any change under src/parser/, src/tld.rs, src/tld/ or src/ast.rs, and when an AST looks different from what Rocket.Chat renders.
---

# Verify upstream equivalence

**Property.** For every input the check generates and each of seven option sets, `parse_at(input, options, clock)` serialized to JSON equals what `parse(input, options)` from `@rocket.chat/message-parser` 0.33.0 returns on Node at the same clock and timezone. Key order does not matter; numbers compare as JavaScript numbers. If upstream throws, or Rust panics, that counts as a mismatch.

**Harm if broken.** A message renders differently in clients that use this crate than in Rocket.Chat: bold becomes literal asterisks, a link is lost or points somewhere else, a mention or emoji is missing.

## Run

Install the pinned upstream package once. Its `node_modules` is gitignored:

```sh
(cd benchmarks/upstream && bun install --frozen-lockfile)
TZ=UTC cargo test --release --test differential -- --ignored --nocapture
```

This takes about 35 seconds and needs `node` on `PATH`. To keep `node_modules` out of the checkout, copy `benchmarks/upstream/package.json` and `bun.lock` into a `mktemp -d` directory, run `bun install --frozen-lockfile` there, and set `UPSTREAM_PARSER_DIR` to that directory. The oracle refuses to run on any version other than 0.33.0.

The three tests in `tests/differential.rs` are `#[ignore]`d, so plain `cargo test` skips them. Each one prints `N inputs x 7 option sets: every AST matches upstream` when it passes:

- `every_short_character_string_matches_upstream`: every string of 1 to 3 characters over `WIDE_ALPHABET` (23 markup characters, a letter, a digit, space and newline), plus every 4-character string over `CORE_ALPHABET` (13). 41,280 inputs.
- `every_short_token_sequence_matches_upstream`: every sequence of 1 to 3 entries of `TOKENS` (47 multi-character markup pieces: `<t:…:R>`, `- [ ] `, `http://a.b`, `a@b.co`, `color:#f00`, `:smile:`, `\(`, `😀` and so on). 104,758 distinct inputs.
- `random_token_sequences_match_upstream`: `DIFFERENTIAL_CASES` (default 20,000) sequences of 1 to 12 pieces, each a `TOKENS` entry or a `WIDE_ALPHABET` character, from the seed `DIFFERENTIAL_SEED` (default `0x5eed0033`). Raise the count and change the seed for a deeper run.

The option sets are in `option_sets()`: default, `emoticons`, `colors`, `katex.dollarSyntax`, `katex.parenthesisSyntax`, `customDomains: ["a", "local"]`, and all of them together. The clock is `CLOCK_SECONDS` (2025-07-22T00:00:00Z) on both sides: Rust gets it through `parse_at`, and `tests/differential/oracle.mjs` replaces Node's `Date` so `new Date()` and `Date.now()` return it. Both processes share the `TZ` you set. Run once more with `TZ=America/New_York` when a change touches `src/parser/timestamp.rs`, because times without an offset resolve in the local zone.

## Reading a failure

The panic message gives the number of failing (input, option set) pairs. It then minimizes up to 400 of them by deleting characters for as long as the input still mismatches, and prints each distinct result with its option set, upstream's JSON and Rust's JSON. Start from the shortest one.

- **Real bug.** The Rust AST differs and the README's "Differences from upstream" section does not list the case. Find the upstream rule in the grammar (the oracle parses with the same `dist/messageParser.js` that `benchmarks/upstream` installs), then the matching rule in `src/parser/`. Add the minimized input to the fitting `tests/fixtures/upstream/*.json` case list or a focused test in `tests/` when you fix it.
- **Documented difference.** Empty input (upstream throws, Rust returns `[]`), lists deeper than 64 levels, lone surrogates (Rust `&str` cannot hold one, so the generator never makes them), and the `work-report` feature. The generators never produce the first three. If you deliberately add another difference, write it in the README first, then keep the generators from producing it instead of filtering results.
- **Harness error.** `upstream: throws: …` on a non-empty input is a real mismatch, not a harness error: upstream's grammar can fail where Rust returns an AST. A harness error looks like `oracle exited early`, a JSON parse panic, or the install message; fix the setup, not the property.
- **Clock or zone.** A mismatch only in a `<t:…>` value that moves with `TZ` or with the clock means one side read the real clock or zone. Check that `parse_at` gets `clock()` and that the oracle still installs `FixedDate` before it imports the parser.

Do not weaken the comparison, drop an option set, or shrink an alphabet to make a run pass. A mismatch stays a finding until the code or the README changes.

## Coverage

| Check | Code it covers |
| --- | --- |
| AST shape and serialization | `Node` and its serde attributes in `src/ast.rs` |
| Every rule reachable from the alphabets and tokens | `src/parser/{mod,blocks,inline,links,emoji,timestamp,reduce,repetition,memo,chars,char_tables}.rs` |
| Option handling | `Options`, `KatexOptions` in `src/lib.rs`; every `self.options` read in `src/parser/` |
| Autolinks and TLDs | `auto_link`, `resolve_href` in `src/parser/links.rs`; `is_linkable` in `src/tld.rs` for the hosts in `TOKENS` (`b.com`, `a.b`, `rocket.chat`, `b.co`) and `customDomains` |
| Emoticons and shortcodes | `src/parser/emoji.rs`, `join_emoji` in `src/parser/reduce.rs` |
| Relative and absolute timestamps | `src/parser/timestamp.rs`, at one clock and the zone in `TZ` |

`.claude/skills/verify-linear-parse` covers work and stack depth; this skill covers only output.

**Sync commit:** `6d5260b`

When covered code changes, compare it against the sync commit (`git diff 6d5260b -- src`). For each new rule, option, marker character or token, check that `WIDE_ALPHABET`, `CORE_ALPHABET` or `TOKENS` can produce it, and add it if not. Run the check, then move the sync commit to the commit you confirmed.

## What this does not prove

- It is bounded, not total. Exhaustive coverage ends at 3 characters over `WIDE_ALPHABET`, 4 over `CORE_ALPHABET`, and 3 pieces over `TOKENS`. Longer inputs are sampled at random.
- Characters outside the alphabets and tokens are not exercised: other Unicode emoji and their modifiers, most of the TLD list, `\r\n` line endings, tabs, and non-ASCII letters in links and mentions. `tests/emoji.rs` covers the emoji table and `src/tld/tests.rs` covers host linkability against upstream data.
- One clock and one zone per run. Daylight saving edges depend on `TZ`; `tests/local_time.rs` covers them.
- Lists nested past 64 levels and inputs over a few hundred characters are never generated.
