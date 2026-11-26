export class ParseCall {
	constructor(
		readonly input: string,
		readonly options: unknown,
	) {}
}

export class UtilityCallError extends Error {}

type ThrowCheckRecorder = (call: ParseCall) => void;

let throwCheckRecorder: ThrowCheckRecorder | undefined;

export const withThrowCheckRecorder = (recorder: ThrowCheckRecorder, run: () => void) => {
	throwCheckRecorder = recorder;
	try {
		run();
	} finally {
		throwCheckRecorder = undefined;
	}
};

export const parse = (input: string, options?: unknown) => {
	const call = new ParseCall(input, options);
	throwCheckRecorder?.(call);
	return call;
};

export const parser = parse;

const utility =
	(name: string) =>
	(..._arguments: unknown[]): never => {
		throw new UtilityCallError(`direct call to src/utils.${name}, not observable through parse()`);
	};

export const isNodeOfType = utility('isNodeOfType');
export const asciiList = utility('asciiList');
export const autoLink = utility('autoLink');
export const reducePlainTexts = utility('reducePlainTexts');
export const plain = utility('plain');
export const emoji = utility('emoji');
export const timestamp = utility('timestamp');
