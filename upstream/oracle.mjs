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

const { parse } = await import('@rocket.chat/message-parser');

const respond = (response) => process.stdout.write(`${JSON.stringify(response)}\n`);

const answer = (request) => {
	const { input, options, now } = JSON.parse(request);
	pinnedNow = now;
	try {
		respond({ nodes: parse(input, options) });
	} catch (error) {
		respond({ error: String(error) });
	}
};

let pending = '';
process.stdin.setEncoding('utf8');
for await (const chunk of process.stdin) {
	const requests = (pending + chunk).split('\n');
	pending = requests.pop();
	requests.forEach(answer);
}
