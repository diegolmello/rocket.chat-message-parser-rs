---
name: verify-linear-parse
description: Check that the message parser does linear work and bounded recursion. Use before committing any change under src/parser/, to the memo tables, or to a loop or lookahead in the grammar port, and when a parse looks slow or overflows the stack.
---

# Verify linear parse work

**Property.** For every input, each memoized rule is evaluated at most once per (position, skip flags), and total work (`ParseReport::work()`: input reads, including every unit `Parser::text` copies, plus rule calls) stays within a constant times input length. Emoticon lookahead chains of any length parse on an 8 MiB stack, and list nesting depth adds no parser stack frames.

**Harm if broken.** A user sends a message of repeated markup (`*_~[$:`, `[[[[`, `:):):)`), and every client that renders it freezes for seconds, or the process aborts on a stack overflow (50,000 chained emoticons, or thousands of indented list lines).

## Run

```sh
cargo test --release --test linear_work -- --nocapture
```

It takes about 10 seconds. In a debug build, `cargo test` skips the seed test; always run the release command above.

The four tests:

- `every_short_input_evaluates_each_rule_position_once_within_the_work_limit`: every string up to 4 characters over ``* _ ~ [ ] ( $ : ` < \n a``, with default options and with every option on. Fails on `repeated_evaluations > 0` or work above `WORK_PER_UNIT_LIMIT × (length + 1)`.
- `repeated_seeds_grow_linearly`: every seed in `seeds()` (all 1–2 character strings over `SEED_ALPHABET`, all 3 character strings over `CORE_SEED_ALPHABET`, `KNOWN_SEEDS`, and `PREFIXED_SEEDS`, whose prefix is written once before the repetitions, so one list item can own an unbounded run of nested lines) repeated a whole number of times, to at least 400 and then 8 times that length. Fails when work per unit grows by more than `DENSITY_GROWTH_LIMIT` (2.0) or exceeds `WORK_PER_UNIT_LIMIT` (400). On a pass it prints the worst growth and density; on the sync commit these were 1.84x (``"* `"``) and 240 per unit (`"*_~[$:"`).
- `long_emoticon_chains_parse_within_a_main_thread_stack`: 50,000 chained `:)` emoticons, bare and inside bold and italic, on an 8 MiB thread.
- `deeply_nested_lists_parse_within_a_small_stack`: 2,000 lines, each indented one space more than the last, parsed on a 1 MiB thread; checks the result is 2,000 levels deep. It walks the tree by moving each level out, so the result's drop does not recurse.

## Reading a failure

Each violation names the option set, the seed, the work at both lengths and the growth. To find the rule responsible, temporarily count reads and memo calls per innermost memoized rule. Push `stringify!($table)` onto a stack around the body in `memoized!` (`src/parser/memo.rs`), and count in `Parser::at` (`src/parser/mod.rs`) against the top of the stack. Drop the instrumentation afterwards.

- **Real bug.** Work per unit keeps growing as the seed is repeated further: 2x the repetitions, about 2x the density. That means some loop or lookahead restarts from many positions and walks far each time. Known shapes and their fixes:
  - A `run1`/`while` loop over a character class called from inside its own run. Use `run_end(Run::…)` in `src/parser/repetition.rs`.
  - A content loop restarted at every opening marker. Use `repetition_end` plus `repetition_items`, so loop ends are memoized and nodes are built only after the closer matches.
  - A string built from the source before the enclosing rule has matched, on a path tried from many starts. Return `Piece::Text` or `Href::Source` (a range) and copy with `self.text` only once the match is certain, as `repetition_items` and `resolve_href` do.
  - A loop over lines that rescans earlier lines. `PREFIXED_SEEDS` catch this in `nested_list_lines`.
  - An unmemoized recursive lookahead. Memoize it, and evaluate the chain backwards like `evaluate_emoticon_chain_backwards` in `src/parser/inline.rs`.
- **Measurement artefact.** Some seeds switch between two linear regimes depending on the repetition count; `"* _"` costs 164 or 102 per repetition. Confirm by printing work per repetition across several counts: a flat series is linear. Never raise a limit without that series in hand.
- **Repeated evaluations.** A memo key misses part of the rule's real input. For example, a rule reads options or flags that are not in its `(position, flags)` key, or a body was called outside `memoized!`.

## Coverage

| Check | Code it covers |
| --- | --- |
| Memo discipline | `src/parser/memo.rs` (`memo_tables!`, `memoized!`), every `memoized!` call site in `src/parser/*.rs` |
| Loop ends | `src/parser/repetition.rs` (`steps_end`, `repetition_end`, `run_end`, `repetition_items`, `Loop`, `Repetition`, `Run`, `Piece`) |
| Deferred copies | `Parser::text` (counted reads) and `Parser::character` (keeps a surrogate pair whole; `KNOWN_SEEDS` `"~🀄"`, `"~x🇧~"` reach it) in `src/parser/mod.rs`; `Href`, `resolve_href`, `link_ref`, `markdown_link_ref`, and `phone`, `phone_number`, `phone_prefix` (which return only an end) in `src/parser/links.rs` |
| Precomputed scans | `Scans` and `email_guard`, `url_guard`, `block_katex_end`, `inline_katex_end`, `extra_run_end` in `src/parser/mod.rs` |
| Rule bodies reached by the seeds | `src/parser/{blocks,inline,links,emoji,timestamp}.rs` |
| Stack depth | `evaluate_emoticon_chain_backwards`, `inline_emoticon`, `EmphasisContent`, `emphasis_content_item`, `preferential_item`, `preferential_item_uncached` in `src/parser/inline.rs`; `group_nested_lines`, `NestingLevel`, `close_level`, `attach_nested`, `nested_lists`, `nested_list_lines` in `src/parser/blocks.rs` |
| Nested list lines | `nested_list_lines` (memoizes flat `NestedLine`s, never the grouped tree) and its callers `task`, `ordered_list_item`, `unordered_list_item` in `src/parser/blocks.rs` |

Skip flags are passed by value as `Flags`, never stored on `Parser`, so they are restored after every rule by construction; no test covers this.

**Sync commit:** `f9e8278`

When covered code changes, compare it against the sync commit (`git diff f9e8278 -- src/parser`). Check that the alphabets and `KNOWN_SEEDS` still reach each new loop, lookahead or rule, and add seeds that do. Run the check, then move the sync commit to the commit you confirmed.

## What this does not prove

- Work is checked on generated inputs, not proven for all inputs. A superlinear path reachable only through strings longer than the seeds, or through characters outside the alphabets, can pass.
- The density check catches a superlinear term once it is about half the work at 8x length. A small quadratic term can pass.
- Clones of memoized results on a memo hit are not counted as work, and neither is `group_nested_lines`, which reruns on every `nested_lists` call, memo hits included. Both cost the size of the memoized value.
- `Node`'s derived `Clone`, `Drop` and `Serialize` recurse once per nesting level. Dropping a 2,000-level list tree fits a 1 MiB stack; 10,000 levels (about 50 million characters of indentation) overflows it. Serializing to JSON on a 2 MiB stack overflows at about 6,500 levels. The depth test moves levels out to avoid this, so it does not cover dropping or cloning the result.
- Wall-clock time grows a little faster than work: about 1.1–1.2x per doubling of length, measured between 4k and 64k characters, because the memo tables outgrow the CPU caches. `cargo run --release --example benchmark` shows the timings.
- Output correctness is `tests/upstream.rs`'s job, not this check's. Local-time resolution in `src/parser/timestamp.rs` does constant work per timestamp; the ISO date seeds in `KNOWN_SEEDS` reach it in the clock's zone, but its daylight saving branches depend on `TZ`, and `tests/local_time.rs` covers their output.
