import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { parse } from '@rocket.chat/message-parser';

const repositoryRoot = fileURLToPath(new URL('../', import.meta.url));
const { pattern, samples, scenarios } = JSON.parse(
	readFileSync(new URL('../benchmarks/scenarios.json', import.meta.url), 'utf8'),
);

const markupSoup = (length) => pattern.repeat(Math.ceil(length / pattern.length)).slice(0, length);

const median = (durations) => [...durations].sort((left, right) => left - right)[Math.floor(durations.length / 2)];

const measureUpstream = ({ length, options }) => {
	const input = markupSoup(length);
	parse(input, options);
	const durations = [];
	for (let sample = 0; sample < samples; sample += 1) {
		const started = performance.now();
		parse(input, options);
		durations.push(performance.now() - started);
	}
	return median(durations);
};

const measureRust = () =>
	JSON.parse(
		execFileSync(
			'cargo',
			['run', '--quiet', '--release', '--features', 'work-report', '--example', 'benchmark', '--', '--json'],
			{ cwd: repositoryRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
		),
	);

const milliseconds = (value) => `${value.toFixed(2)} ms`;

const rustRows = measureRust();
console.log(
	['length', 'options', 'rust', 'upstream', 'speedup'].map((heading, index) => heading.padStart(index < 2 ? 7 : 12)).join(' '),
);
scenarios.forEach((scenario, index) => {
	const rust = rustRows[index].medianMilliseconds;
	const upstream = measureUpstream(scenario);
	console.log(
		[
			String(scenario.length).padStart(7),
			scenario.label.padStart(7),
			milliseconds(rust).padStart(12),
			milliseconds(upstream).padStart(12),
			`${(upstream / rust).toFixed(1)}x`.padStart(12),
		].join(' '),
	);
});
