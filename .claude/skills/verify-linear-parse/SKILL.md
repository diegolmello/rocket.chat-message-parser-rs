---
name: verify-linear-parse
description: Check that the message parser does linear work and bounded recursion. Use before committing any change under src/parser/, to the memo tables, or to a loop or lookahead in the grammar port, and when a parse looks slow or overflows the stack.
metadata:
  covers:
    - 'src/parser/**'
    - src/lib.rs
    - tests/common/mod.rs
  check: cargo test --release --features work-report --test linear_work && cargo test --release --test stack_depth
---

# Verify linear parse work

**Property.** For every input, each memoized rule is evaluated at most once per (position, skip flags), and total work (`ParseReport::work()`: input reads, including every unit `Parser::text` copies, plus rule calls) stays within a constant times input length. Emoticon lookahead chains of any length parse on an 8 MiB stack, spoilers that end in an emoticon chain on a 2 MiB stack, and list nesting depth adds no parser stack frames.

**Harm if broken.** A user sends a message of repeated markup (`*_~[$:`, `[[[[`, `:):):)`), and every client that renders it freezes for seconds, or the process aborts on a stack overflow (50,000 chained emoticons, `||:)` repeated past about 1,000 times, or thousands of indented list lines).

## Run

```sh
cargo test --release --features work-report --test linear_work -- --nocapture
cargo test --release --test stack_depth
```

They take about 15 seconds. `tests/linear_work.rs` reads `ParseReport` through `parse_with_report`, which exists only with the `work-report` feature, so plain `cargo test` skips that file. A debug build also skips the seed test; always run the release commands above. Both files take `fixed_clock` from `tests/common/mod.rs`, and `tests/linear_work.rs` builds its alphabets' strings with `strings_over` from the same file.

The tests in `tests/linear_work.rs`:

- `every_short_input_evaluates_each_rule_position_once_within_the_work_limit`: every string up to 4 characters over ``* _ ~ [ ] ( $ : ` < \n a``, with default options and with every option on. Fails on `repeated_evaluations > 0` or work above `WORK_PER_UNIT_LIMIT × (length + 1)`.
- `repeated_seeds_grow_linearly`: every seed in `seeds()` (all 1–2 character strings over `SEED_ALPHABET`, all 3 character strings over `CORE_SEED_ALPHABET`, `KNOWN_SEEDS`, and `PREFIXED_SEEDS`, whose prefix is written once before the repetitions, so one list item can own an unbounded run of nested lines) repeated a whole number of times, to at least a minimum length and then 8 times that length. The minimum is `MINIMUM_LENGTH` (400) for the generated seeds and `KNOWN_SEED_MINIMUM_LENGTH` (4,000) for `KNOWN_SEEDS` and `PREFIXED_SEEDS`: a small quadratic term slips under the growth limit from 400 units and shows from 4,000 (`"||-___-"` with the spoiler content memo broken grows 1.6x from 400 units and 4.5x from 4,000). Fails when work per unit grows by more than `DENSITY_GROWTH_LIMIT` (2.0) or exceeds `WORK_PER_UNIT_LIMIT` (400). On a pass it prints the worst growth and density; on the sync commit these were 1.84x (``"* `"``) and 251 per unit (`"*_~[$:"`, which levels off near 252 from 8,000 units up).

The tests in `tests/stack_depth.rs`, which need no feature:

- `long_emoticon_chains_parse_within_a_main_thread_stack`: 50,000 chained `:)` emoticons, bare and inside bold and italic, on an 8 MiB thread.
- `spoilers_ending_in_emoticons_parse_within_a_spawned_thread_stack`: `||:)` repeated 25,000 times, bare and after `*`, `_` and `![`, `||x<3` and `||>:-)` repeated 20,000 times, and `||-___-` repeated 20,000 times, with emoticons on, on a 2 MiB thread. Each emoticon looks ahead into the next spoiler, so the recursion grows by one spoiler per repetition until `within_emoticon_lookahead` stops it at `MAXIMUM_EMOTICON_LOOKAHEAD_DEPTH` (500) levels, about 1.3 MiB in a release build. The test asserts how many emoji each input yields: past the cap one emoticon in 500 becomes plain text (24,950 of 24,999 for `||:)` × 25,000), so a parse that gives up early, or a cap that moved, fails. Debug frames are several times larger, so a debug build skips this test. The work of `||-___-` is checked by its `KNOWN_SEEDS` entry, which reaches the spoiler content memo (`Repetition::Spoiler` through `repetition_end`): italic matches `_-||-_` across every `||`, so each spoiler's content would otherwise walk to the end of the input.
- `deeply_nested_lists_stop_nesting_at_the_depth_limit_and_keep_every_item`: 2,000 lines, each indented one space more than the last, on a 2 MiB thread (Rust's default for spawned threads). It parses, serializes, clones and drops the result. It checks the tree stops at 64 levels (`MAXIMUM_NESTED_LEVELS` in `src/parser/blocks.rs` plus the top list), the deepest list holds the remaining 1,937 items, and all 2,000 items are kept.

## Reading a failure

Each violation names the option set, the seed, the work at both lengths and the growth. To find the rule responsible, temporarily count reads and memo calls per innermost memoized rule. Push `stringify!($table)` onto a stack around the body in `memoized!` (`src/parser/memo.rs`), and count in `Parser::at` (`src/parser/mod.rs`) against the top of the stack. Drop the instrumentation afterwards.

- **Real bug.** Work per unit keeps growing as the seed is repeated further: 2x the repetitions, about 2x the density. That means some loop or lookahead restarts from many positions and walks far each time. Known shapes and their fixes:
  - A `run1`/`while` loop over a character class called from inside its own run. Use `run_end(Run::…)` in `src/parser/repetition.rs`.
  - A content loop restarted at every opening marker. Use `repetition_end` plus `repetition_items`, so loop ends are memoized and nodes are built only after the closer matches.
  - A string built from the source before the enclosing rule has matched, on a path tried from many starts. Return `Piece::Text` or `Href::Source` (a range) and copy with `self.text` only once the match is certain, as `repetition_items` and `resolve_href` do.
  - A loop over lines that rescans earlier lines. `PREFIXED_SEEDS` catch this in `nested_list_lines`.
  - An unmemoized recursive lookahead. Memoize it, and evaluate the chain backwards like `evaluate_emoticon_chain_backwards` in `src/parser/inline.rs`.
  - A lookahead into a sibling container whose content holds the same lookahead. Memoize the lookahead's target and bound its depth (`within_emoticon_lookahead`); never pre-evaluate containers the forward parse may not reach. Evaluating them costs their whole content: `KNOWN_SEEDS` `":)||a.b"`, `":D||a.b"`, `"x<3||a.b"`, `"||~-___-"`, `"||*-___-"`, `"-___-||a"` and `"-___-||*"` grew 2.7x to 8.0x under the backward spoiler pass that `d340125` removed.
  - A block that scans every following line for its closer, restarted on every line. Memoize the end of the scan per start (`Run::SpoilerParagraphs` through `run_end` for `block_spoiler`) or index the closers once per parse (`long_code_closes` in `Scans` for `long_code`); `KNOWN_SEEDS` `"||\n\r"`, `"||\n||a.b\n"`, `"````x\n"`, `"````\n```\n"` and `"`````\n````x\n"`, and `PREFIXED_SEEDS` `("||", "\na")` and `("````", "\n```")` (one unclosed block before an unbounded run of lines) reach them. A `block_spoiler` that loops `spoiler_paragraph_step` itself fails on `"||\n\r"` (6.3x); a `next_long_code_close` that scans forward line by line fails on `"````x\n"` (7.4x) and `"`````\n````x\n"`.
- **Measurement artefact.** Some seeds switch between two linear regimes depending on the repetition count; `"* _"` costs 164 or 102 per repetition. Confirm by printing work per repetition across several counts: a flat series is linear. Never raise a limit without that series in hand.
- **Repeated evaluations.** A memo key misses part of the rule's real input. For example, a rule reads options or flags that are not in its `(position, flags)` key, or a body was called outside `memoized!`.

## Coverage

| Check | Code it covers |
| --- | --- |
| Memo discipline | `src/parser/memo.rs` (`memo_tables!`, `memoized!`), every `memoized!` call site in `src/parser/*.rs` |
| Loop ends | `src/parser/repetition.rs` (`steps_end`, `repetition_end`, `run_end`, `repetition_items`, `Loop`, `Repetition`, `Run`, `Piece`) |
| Deferred copies | `Parser::text` (counted reads) and `Parser::character` (keeps a surrogate pair whole; `KNOWN_SEEDS` `"~🀄"`, `"~x🇧~"` reach it) in `src/parser/mod.rs`; `Href`, `resolve_href`, `link_ref`, `markdown_link_ref`, and `phone`, `phone_number`, `phone_prefix` (which return only an end) in `src/parser/links.rs` |
| Precomputed scans | `Scans`, `scan` (counts one read per lookup) and its callers `email_guard`, `url_guard`, `block_katex_end`, `inline_katex_end`, `extra_run_end` in `src/parser/mod.rs` |
| Block closers | `block_spoiler`, `spoiler_paragraph_step`, `long_code`, `next_long_code_close`, `fence_close`, `long_code_closes` in `src/parser/blocks.rs`; `Run::SpoilerParagraphs` in `src/parser/repetition.rs`; `Scans::long_code_closes` in `src/parser/mod.rs` |
| Rule bodies reached by the seeds | `src/parser/{blocks,inline,links,emoji,timestamp}.rs`, from the top-level loop `start` in `src/parser/blocks.rs` |
| Stack depth | `MAXIMUM_NESTED_LEVELS` in `src/parser/blocks.rs`; `spoiler`, `spoiler_content_item`, `MAXIMUM_EMOTICON_LOOKAHEAD_DEPTH`, `within_emoticon_lookahead` (and `emoticon_lookahead_depth` on `Parser` in `src/parser/mod.rs`), `evaluate_emoticon_chain_backwards` (stops at the first position already in the `MemoTable` it is given), `inline_emoticon`, `EmphasisContent` (`preferential_table` picks that table), `emphasis_content_item`, `preferential_item`, `preferential_item_uncached` in `src/parser/inline.rs`; `group_nested_lines`, `NestingLevel`, `close_level`, `attach_nested`, `nested_lists`, `nested_list_lines` in `src/parser/blocks.rs` |
| Nested list lines | `nested_list_lines` (memoizes flat `NestedLine`s, never the grouped tree), `nested_list_line_item`, `list_line_item`, and the top-level loop `list_of_marker` that calls `nested_lists` in `src/parser/blocks.rs` |

Skip flags are passed by value as `Flags`, never stored on `Parser`, so they are restored after every rule by construction; no test covers this.

**Sync commit:** `42daa27`

When covered code changes, compare it against the sync commit (`git diff 42daa27 -- src/parser src/lib.rs tests/common`). Check that the alphabets and `KNOWN_SEEDS` still reach each new loop, lookahead or rule, and add seeds that do. Run the check, then move the sync commit to the commit you confirmed.

## What this does not prove

- Work is checked on generated inputs, not proven for all inputs. A superlinear path reachable only through strings longer than the seeds, or through characters outside the alphabets, can pass.
- The density check catches a superlinear term once it is about half the work at 8x length. A small quadratic term can pass.
- Clones of memoized results on a memo hit are not counted as work, and neither is `group_nested_lines`, which reruns on every `nested_lists` call, memo hits included. Both cost the size of the memoized value.
- `Node`'s derived `Clone`, `Drop` and `Serialize` recurse once per nesting level, so the depth cap is what keeps them safe. A debug build's `Serialize` takes about 16 KB of stack per level: about 60 levels fit in 1 MiB. Raising `MAXIMUM_NESTED_LEVELS` past about 120 breaks debug serialization on a 2 MiB thread. Bold, italic and strike nesting is bounded by the skip flags; no other node nests without bound.
- Wall-clock time grows a little faster than work: about 1.1–1.2x per doubling of length, measured between 4k and 64k characters, because the memo tables outgrow the CPU caches. `cargo run --release --features work-report --example benchmark` shows the timings.
- Output correctness is `tests/upstream.rs`'s job, not this check's. Local-time resolution in `src/parser/timestamp.rs` does constant work per timestamp; the ISO date seeds in `KNOWN_SEEDS` reach it in the clock's zone, but its daylight saving branches depend on `TZ`, and `tests/local_time.rs` covers their output.
