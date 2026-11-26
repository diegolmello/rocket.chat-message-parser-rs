import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const [upstreamDirectory] = process.argv.slice(2);
if (!upstreamDirectory) {
	throw new Error('usage: node oracle.mjs <directory holding node_modules/@rocket.chat/message-parser>');
}

const SystemDate = Date;
let pinnedNow = SystemDate.now();

class PinnedDate extends SystemDate {
	constructor(...dateArguments) {
		if (dateArguments.length === 0) {
			super(pinnedNow);
		} else {
			super(...dateArguments);
		}
	}

	static now() {
		return pinnedNow;
	}
}

globalThis.Date = PinnedDate;

const requireFromUpstream = createRequire(join(upstreamDirectory, 'package.json'));
const packageJson = JSON.parse(
	readFileSync(join(upstreamDirectory, 'node_modules', '@rocket.chat', 'message-parser', 'package.json'), 'utf8'),
);
if (packageJson.version !== '0.33.0') {
	throw new Error(`expected @rocket.chat/message-parser 0.33.0, found ${packageJson.version}`);
}
const { parse } = await import(pathToFileURL(requireFromUpstream.resolve('@rocket.chat/message-parser')).href);

const answer = (request) => {
	const { input, options, now } = JSON.parse(request);
	pinnedNow = now;
	try {
		return { nodes: parse(input, options) };
	} catch (error) {
		return { error: String(error) };
	}
};

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
	process.stdout.write(`${JSON.stringify(answer(line))}\n`);
}
