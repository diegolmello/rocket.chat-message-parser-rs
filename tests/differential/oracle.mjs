import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const [upstreamDirectory, clockMilliseconds] = process.argv.slice(2);
if (!upstreamDirectory || !clockMilliseconds) {
	throw new Error('usage: node oracle.mjs <directory holding node_modules/@rocket.chat/message-parser> <clock in ms>');
}

const fixedNow = Number(clockMilliseconds);
const RealDate = globalThis.Date;
class FixedDate extends RealDate {
	constructor(...values) {
		super(...(values.length === 0 ? [fixedNow] : values));
	}

	static now() {
		return fixedNow;
	}
}
globalThis.Date = FixedDate;

const requireFromUpstream = createRequire(join(upstreamDirectory, 'package.json'));
const packageJson = JSON.parse(
	readFileSync(join(upstreamDirectory, 'node_modules', '@rocket.chat', 'message-parser', 'package.json'), 'utf8'),
);
if (packageJson.version !== '0.33.0') {
	throw new Error(`expected @rocket.chat/message-parser 0.33.0, found ${packageJson.version}`);
}
const { parse } = await import(pathToFileURL(requireFromUpstream.resolve('@rocket.chat/message-parser')).href);

const answer = ({ input, options }) => {
	try {
		return { ast: parse(input, options) };
	} catch (error) {
		return { error: String(error?.message ?? error) };
	}
};

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
	process.stdout.write(`${JSON.stringify(answer(JSON.parse(line)))}\n`);
}
