// util/near-logger-proxy.ts
import { AsyncLocalStorage } from 'async_hooks';

type AnyFn = (...args: any[]) => any;

export type NearAccountLike = { accountId: string };

type Logger = (info: {
  accountId: string;
  prop: string | symbol;
  args?: unknown[];
  result?: unknown;
  error?: unknown;
  depth: number;
}) => void;

export interface LoggingOptions {
  onlyTopLevel?: boolean;              // default true
  include?: (string | RegExp | ((p: PropertyKey) => boolean))[];
  exclude?: (string | RegExp | ((p: PropertyKey) => boolean))[];
  logger?: Logger;                     // custom logger (e.g., use t.log)
  maxLen?: number;                     // pretty print truncation
}

const als = new AsyncLocalStorage<{ depth: number }>();

const isPromise = (x: any): x is Promise<unknown> =>
  !!x && typeof x.then === 'function';

const matches = (prop: PropertyKey, m: string | RegExp | ((p: PropertyKey) => boolean)) =>
  typeof m === 'string' ? prop === m
    : m instanceof RegExp ? m.test(String(prop))
      : m(prop);

function pretty(value: unknown, maxLen = 10_000): string {
  // BigInt-safe JSON stringify with indentation
  let s: string;
  try {
    s = JSON.stringify(value, (_k, val) =>
      typeof val === 'bigint' ? val.toString() : val,
      2 // <- preserve indentation
    );
  } catch {
    s = String(value);
  }

  // Truncate extremely large values
  if (s.length > maxLen) s = s.slice(0, maxLen) + "…";

  return s;
}

// Indent all lines except the first, so it aligns nicely under the marker
function indent(value: string, spaces = 2): string {
  const pad = ' '.repeat(spaces);
  const lines = value.split('\n');
  return lines
    .map((line) => pad + line)
    .join('\n');
}

// Default logger: puts accountId first and uses your glyphs.
const defaultLogger: Logger = ({ accountId, prop, args, result, error }) => {
  const name = String(prop);

  const bold = (s: string) => `\x1b[1m${s}\x1b[0m`;
  const gray = (s: string) => `\x1b[90m${s}\x1b[0m`;
  const green = (s: string) => `\x1b[32m${s}\x1b[0m`;
  const red = (s: string) => `\x1b[31m${s}\x1b[0m`;

  // Command call (keep plain)
  if (args) {
    console.log(`➤ ${bold(accountId)} ${bold(name)}(${pretty(args)})`);
  }

  // Response or error
  if (error !== undefined) {
    // Header line in GRAY
    console.log(`  ↩ ${gray(`${bold(accountId)} ✖ ${bold(name)}`)}:`);

    // Body in RED, indented
    console.log(indent(red(pretty(error))));
  }

  if (result !== undefined) {
    // Header line in GRAY
    console.log(`  ↩ ${gray(`${bold(accountId)} → ${bold(name)}`)}:`);

    // Body in GREEN, indented
    console.log(indent(green(pretty(result))));
  }
};

export function withLogging<T extends NearAccountLike & object>(
  target: T,
  opts: LoggingOptions = {}
): T {
  const {
    onlyTopLevel = true,
    include,
    exclude,
    logger = defaultLogger,
    maxLen = 10_000,
  } = opts;

  const handler: ProxyHandler<T> = {
    get(obj, prop, receiver) {
      const value = Reflect.get(obj, prop, receiver);

      // Pass through non-functions unchanged
      if (typeof value !== 'function') return value;

      const shouldLog = (): boolean => {
        if (exclude?.some((m) => matches(prop, m))) return false;
        if (include && !include.some((m) => matches(prop, m))) return false;
        return true;
      };

      const wrapped = function (this: unknown, ...args: any[]) {
        const store = als.getStore();
        const depth = store?.depth ?? 0;

        // Resolve accountId from `this` (proxy) or original object
        const self: any = this ?? receiver;
        const accountId: string =
          (self?.accountId ?? (obj as any).accountId) as string;

        const logNow = shouldLog() && (!onlyTopLevel || depth === 0);

        const invoke = () => (value as AnyFn).apply(this, args);

        if (!logNow) {
          // Still increment depth so nested-of-nested is tracked
          return als.run({ depth: depth + 1 }, invoke);
        }

        // Pre-call
        logger({ accountId, prop, args, depth });

        try {
          const res = als.run({ depth: depth + 1 }, invoke);
          if (isPromise(res)) {
            return (res as Promise<unknown>)
              .then((val) => {
                logger({ accountId, prop, result: val, depth });
                return val;
              })
              .catch((err) => {
                logger({ accountId, prop, error: err, depth });
                throw err;
              });
          } else {
            logger({ accountId, prop, result: res, depth });
            return res;
          }
        } catch (err) {
          logger({ accountId, prop, error: err, depth });
          throw err;
        }
      };

      // Keep type shape (overloads preserved by inference on call)
      return wrapped as typeof value;
    },
  };

  return new Proxy(target, handler);
}
