# Upstream test fixtures

`tests/fixtures/upstream/*.json` and `tests/fixtures/emoji/all-emoji.json` are generated from the
`@rocket.chat/message-parser` Jest suite. Regenerate them with:

```sh
bun scripts/fixtures/extract.ts <Rocket.Chat checkout>/packages/message-parser/tests
```
