import type { Logger } from 'pino';

declare global {
  let logger: Logger;
}

export {};
