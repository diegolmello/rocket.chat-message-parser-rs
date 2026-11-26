import { plugin } from 'bun';
import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

import { ParseCall, withThrowCheckRecorder } from './stub';

const OUT_OF_SCOPE: Record<string, string> = {
	'blockSplitter.spec.ts': 'tests the TypeScript BlockSplitter utility, not the grammar',
	'guards.test.ts': 'tests the TypeScript isNodeOfType guards, not the grammar',
	'joinEmoji.test.ts': 'tests the TypeScript reducePlainTexts utility directly, not the grammar',
	'fuzz.test.ts': 'fast-check property test, hand-ported to tests/fuzz.rs with proptest',
};

const AST_MATCHERS = ['toEqual', 'toStrictEqual', 'toMatchObject'] as const;
type AstMatcher = (typeof AST_MATCHERS)[number];

type Case = {
	name: string;
	input: string;
	options?: unknown;
	matcher: AstMatcher | 'notToThrow';
	expected?: unknown;
};

type RecordedCase = Case & { clock?: string };

type TestEntry = { kind: 'test'; name: string; body: () => unknown };

type Block = {
	kind: 'block';
	name: string;
	children: Array<Block | TestEntry>;
	beforeAll: Array<() => unknown>;
	afterAll: Array<() => unknown>;
};

type RunningTest = { name: string; cases: RecordedCase[] };

class UnsupportedExpectation extends Error {}

const newBlock = (name: string): Block => ({ kind: 'block', name, children: [], beforeAll: [], afterAll: [] });

let blockStack: Block[] = [newBlock('')];
let runningTest: RunningTest | undefined;
let fakeClock: string | undefined;
let expectCalls = 0;

const currentBlock = () => blockStack[blockStack.length - 1];

const prettyFormat = (value: unknown): string => {
	if (typeof value === 'string') {
		return `"${value.replace(/["\\]/g, '\\$&')}"`;
	}
	if (value === undefined) {
		return 'undefined';
	}
	return JSON.stringify(value);
};

const formatTitle = (title: string, values: unknown[], rowIndex: number) => {
	const remaining = [...values];
	return title.replace(/%[psdifjoO#%]/g, (placeholder) => {
		if (placeholder === '%%') {
			return '%';
		}
		if (placeholder === '%#') {
			return String(rowIndex);
		}
		if (remaining.length === 0) {
			return placeholder;
		}
		const value = remaining.shift();
		switch (placeholder) {
			case '%p':
				return prettyFormat(value);
			case '%j':
			case '%o':
			case '%O':
				return JSON.stringify(value);
			case '%d':
			case '%i':
				return String(Math.trunc(Number(value)));
			case '%f':
				return String(Number(value));
			default:
				return String(value);
		}
	});
};

const eachRow = (row: unknown): unknown[] => (Array.isArray(row) ? row : [row]);

const registerTest = (name: string, body: (...values: unknown[]) => unknown) => {
	currentBlock().children.push({ kind: 'test', name, body });
};

const registerEach =
	(register: (name: string, body: () => unknown) => void) =>
	(table: unknown[]) =>
	(title: string, body: (...values: unknown[]) => unknown) => {
		table.forEach((row, rowIndex) => {
			const values = eachRow(row);
			register(formatTitle(title, values, rowIndex), () => body(...values));
		});
	};

const describe = (name: string, body: () => void) => {
	const block = newBlock(name);
	currentBlock().children.push(block);
	blockStack.push(block);
	try {
		body();
	} finally {
		blockStack.pop();
	}
};
describe.each = registerEach(describe);

const test = (name: string, body: () => unknown) => registerTest(name, body);
test.each = registerEach(test);

const containsParseCall = (value: unknown): boolean => {
	if (value instanceof ParseCall) {
		return true;
	}
	if (value !== null && typeof value === 'object') {
		return Object.values(value).some(containsParseCall);
	}
	return false;
};

const isSubset = (actual: unknown, expected: unknown): boolean => {
	if (Array.isArray(expected)) {
		return (
			Array.isArray(actual) &&
			actual.length === expected.length &&
			expected.every((element, index) => isSubset(actual[index], element))
		);
	}
	if (expected !== null && typeof expected === 'object') {
		return (
			actual !== null &&
			typeof actual === 'object' &&
			Object.entries(expected).every(([key, value]) => isSubset((actual as Record<string, unknown>)[key], value))
		);
	}
	return Object.is(actual, expected);
};

const recordCase = (recorded: Case) => {
	if (!runningTest) {
		throw new UnsupportedExpectation('expect() called outside a test');
	}
	runningTest.cases.push({ ...recorded, ...(fakeClock ? { clock: fakeClock } : {}) });
};

const parseCallCase = (call: ParseCall, matcher: Case['matcher'], expected?: unknown): Case => ({
	name: runningTest?.name ?? '',
	input: call.input,
	...(call.options !== undefined ? { options: call.options } : {}),
	matcher,
	...(expected !== undefined ? { expected } : {}),
});

const assert = (holds: boolean, negated: boolean, description: string) => {
	if (holds === negated) {
		throw new Error(`assertion failed: ${negated ? 'not ' : ''}${description}`);
	}
};

const matchers = (actual: unknown, negated: boolean): Record<string, unknown> => {
	const astMatcher = (matcher: AstMatcher) => (expected: unknown) => {
		if (containsParseCall(expected)) {
			throw new UnsupportedExpectation(`${matcher} against another parse() result`);
		}
		if (actual instanceof ParseCall) {
			if (negated) {
				throw new UnsupportedExpectation(`negated ${matcher} on a parse() result`);
			}
			recordCase(parseCallCase(actual, matcher, expected));
			return;
		}
		if (containsParseCall(actual)) {
			throw new UnsupportedExpectation(`${matcher} on a value derived from parse()`);
		}
		const holds =
			matcher === 'toMatchObject' ? isSubset(actual, expected) : Bun.deepEquals(actual, expected, matcher === 'toStrictEqual');
		assert(holds, negated, `${matcher}(${JSON.stringify(expected)}) on ${JSON.stringify(actual)}`);
	};

	const plainValue = (matcher: string) => {
		if (containsParseCall(actual)) {
			throw new UnsupportedExpectation(`${matcher} on a parse() result`);
		}
		return actual;
	};

	return {
		toEqual: astMatcher('toEqual'),
		toStrictEqual: astMatcher('toStrictEqual'),
		toMatchObject: astMatcher('toMatchObject'),
		toBe: (expected: unknown) => assert(Object.is(plainValue('toBe'), expected), negated, `toBe(${prettyFormat(expected)})`),
		toHaveLength: (length: number) =>
			assert((plainValue('toHaveLength') as { length: number }).length === length, negated, `toHaveLength(${length})`),
		toBeGreaterThan: (bound: number) =>
			assert((plainValue('toBeGreaterThan') as number) > bound, negated, `toBeGreaterThan(${bound})`),
		toThrow: () => {
			if (typeof actual !== 'function') {
				throw new UnsupportedExpectation('toThrow on a non-function');
			}
			if (!negated) {
				throw new UnsupportedExpectation('expecting parse() to throw');
			}
			withThrowCheckRecorder(
				(call) => recordCase(parseCallCase(call, 'notToThrow')),
				() => actual(),
			);
		},
		get not() {
			return matchers(actual, !negated);
		},
	};
};

const expect = (actual: unknown) => {
	expectCalls += 1;
	return matchers(actual, false);
};

const jest = {
	useFakeTimers: () => {},
	useRealTimers: () => {
		fakeClock = undefined;
	},
	setSystemTime: (time: Date | number | string) => {
		fakeClock = new Date(time).toISOString();
	},
};

Object.assign(globalThis, {
	describe,
	test,
	it: test,
	expect,
	jest,
	beforeAll: (hook: () => unknown) => currentBlock().beforeAll.push(hook),
	afterAll: (hook: () => unknown) => currentBlock().afterAll.push(hook),
});

const testsDirectory = resolve(process.argv[2] ?? '');
if (!process.argv[2] || !existsSync(join(testsDirectory, 'helpers.ts'))) {
	console.error('usage: bun scripts/fixtures/extract.ts <path to packages/message-parser/tests>');
	process.exit(1);
}

const stubPath = join(import.meta.dir, 'stub.ts');
const stubbedModules = new Set(['index.ts', 'utils.ts'].map((fileName) => realpathSync(resolve(testsDirectory, '../src', fileName))));

plugin({
	name: 'record-parse-calls',
	setup(build) {
		build.onLoad({ filter: /\/src\/(index|utils)\.ts$/ }, (args) => {
			if (!stubbedModules.has(realpathSync(args.path))) {
				return undefined;
			}
			return { contents: `export * from ${JSON.stringify(stubPath)};`, loader: 'ts' };
		});
	},
});

const fixturesDirectory = resolve(import.meta.dir, '../../tests/fixtures');
const upstreamFixturesDirectory = join(fixturesDirectory, 'upstream');
const emojiFixturesDirectory = join(fixturesDirectory, 'emoji');
mkdirSync(upstreamFixturesDirectory, { recursive: true });
mkdirSync(emojiFixturesDirectory, { recursive: true });

const packageVersion = (): string => {
	const packageJsonPath = resolve(testsDirectory, '../package.json');
	if (!existsSync(packageJsonPath)) {
		throw new Error(`no package.json at ${packageJsonPath}; pass the upstream package's tests directory`);
	}
	return JSON.parse(readFileSync(packageJsonPath, 'utf8')).version;
};

const writeJson = (path: string, value: unknown) => writeFileSync(path, `${JSON.stringify(value, null, '\t')}\n`);

type Uncaptured = { name: string; reason: string };

const MAX_REASON_LENGTH = 200;

const runBlock = async (block: Block, prefix: string[], captured: RecordedCase[], uncaptured: Uncaptured[]) => {
	const path = block.name ? [...prefix, block.name] : prefix;
	for (const hook of block.beforeAll) {
		await hook();
	}
	for (const child of block.children) {
		if (child.kind === 'block') {
			await runBlock(child, path, captured, uncaptured);
			continue;
		}
		const name = [...path, child.name].join(' > ');
		runningTest = { name, cases: [] };
		try {
			await child.body();
			captured.push(...runningTest.cases);
		} catch (error) {
			const reason = error instanceof Error ? `${error.constructor.name}: ${error.message}` : String(error);
			uncaptured.push({ name, reason: reason.length > MAX_REASON_LENGTH ? `${reason.slice(0, MAX_REASON_LENGTH)}…` : reason });
		} finally {
			runningTest = undefined;
		}
	}
	for (const hook of block.afterAll) {
		await hook();
	}
};

const groupByClock = (cases: RecordedCase[]) => {
	const groups = new Map<string | undefined, Case[]>();
	for (const { clock, ...recorded } of cases) {
		groups.set(clock, [...(groups.get(clock) ?? []), recorded]);
	}
	return [...groups].map(([clock, groupCases]) => ({ ...(clock ? { needsClock: clock } : {}), cases: groupCases }));
};

const fixtureStem = (fileName: string) => fileName.replace(/\.(test|spec)\.ts$/, '');

const testFiles = readdirSync(testsDirectory)
	.filter((fileName) => /\.(test|spec)\.ts$/.test(fileName))
	.sort();

const version = packageVersion();
const summary: Array<Record<string, string | number>> = [];

for (const fileName of testFiles) {
	const source = readFileSync(join(testsDirectory, fileName), 'utf8');
	const staticExpects = source.match(/\bexpect\(/g)?.length ?? 0;
	if (OUT_OF_SCOPE[fileName]) {
		summary.push({ file: fileName, staticExpects, expectCalls: 0, captured: 0, uncapturedTests: 0, note: OUT_OF_SCOPE[fileName] });
		continue;
	}

	blockStack = [newBlock('')];
	fakeClock = undefined;
	expectCalls = 0;
	await import(join(testsDirectory, fileName));

	const captured: RecordedCase[] = [];
	const uncaptured: Uncaptured[] = [];
	await runBlock(blockStack[0], [], captured, uncaptured);

	writeJson(join(upstreamFixturesDirectory, `${fixtureStem(fileName)}.json`), {
		file: fileName,
		upstreamVersion: version,
		groups: groupByClock(captured),
		...(uncaptured.length > 0 ? { uncaptured } : {}),
	});
	summary.push({ file: fileName, staticExpects, expectCalls, captured: captured.length, uncapturedTests: uncaptured.length, note: '' });
}

const { ALL_EMOJI, EMOJIBASE_VERSION } = await import(join(testsDirectory, 'fixtures/allEmoji.ts'));
writeJson(join(emojiFixturesDirectory, 'all-emoji.json'), { emojibaseVersion: EMOJIBASE_VERSION, emoji: ALL_EMOJI });

console.table(summary);
