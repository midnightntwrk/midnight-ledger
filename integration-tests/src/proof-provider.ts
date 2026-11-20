// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import {
  type Transaction,
  LedgerParameters,
  CostModel,
  type PreProof,
  type PreBinding,
  type Proof,
  type SignatureEnabled,
  type ProvingKeyMaterial,
  createProvingPayload,
  createCheckPayload,
  parseCheckResult
} from '@midnightntwrk/ledger';
import { Cache } from 'cache-ts';
import { type ChildProcess, exec /* , execSync */ } from 'node:child_process';
import { createServer } from 'net';
import axiosRetry from 'axios-retry';
import axios from 'axios';
import fs from 'fs';
import path from 'path';
import { Worker } from 'worker_threads';
import { useWasmProving } from './config';

export const cache = new Cache<string, Transaction<SignatureEnabled, Proof, PreBinding>>(256);
let proofServerUrl: string = 'not started';

let serverProcess: ChildProcess;

const findFreePort = (startPort: number): Promise<number> => {
  return new Promise((resolve, reject) => {
    const server = createServer();

    server.listen(startPort, () => {
      const address = server.address();
      const port = address && typeof address === 'object' ? address.port : null;
      if (port !== null) {
        logger.info(`Found free port: ${port}`);
        server.close(() => resolve(port));
      } else {
        reject(new Error('Could not find free port'));
      }
    });

    server.on('error', () => {
      resolve(findFreePort(startPort + 1));
    });
  });
};

export const startProofServerBinary = async (
  serverBinPath: string = '../result/bin/midnight-proof-server',
  serverParams: string = ' -v',
  expectedStartupMessage: string = 'starting service'
): Promise<void> => {
  if (proofServerUrl !== 'not started') {
    return new Promise((resolve, _reject) => {
      resolve();
    });
  }
  const proofServerExec = path.resolve(serverBinPath);
  if (!fs.existsSync(proofServerExec)) {
    throw new Error(`File does not exist: ${proofServerExec}`);
  }
  const portNumber = await findFreePort(Math.floor(Math.random() * (9300 - 6300 + 1)) + 6300);
  proofServerUrl = `http://127.0.0.1:${portNumber}`;
  const command = `${proofServerExec} -p ${portNumber} ${serverParams}`;
  logger.info(`Launching: ${command}`);

  return new Promise((resolve, reject) => {
    serverProcess = exec(command);

    serverProcess.on('exit', (code, signal) => {
      const errorMessage = `[${serverProcess.pid}] Process exited with code: ${code}, signal: ${signal}`;
      logger.error(errorMessage);
      reject(errorMessage);
    });

    serverProcess.on('error', (error) => {
      const errorMessage = `Failed to start server: ${error}`;
      logger.error(errorMessage);
      reject(errorMessage);
    });

    serverProcess.stderr?.on('data', (data) => {
      const errorMessage = `[ProofServer][STDERR][${serverProcess.pid}] ${data}`;
      logger.warn(errorMessage);
      reject(errorMessage);
    });

    serverProcess.stdout?.on('data', (data: Buffer) => {
      const log = data.toString();
      logger.info(`[${serverProcess.pid}][ProofServer] ${log.trim()}`);
      if (log.includes(expectedStartupMessage)) {
        logger.info(`[${serverProcess.pid}] Detected: Server started`);
        resolve();
      }
    });
  });
};

export const stopProofServerBinary = async (): Promise<void> => {
  logger.info(`Stopping server process...`);
  return new Promise<void>((resolve) => {
    logger.info(`Stopping server process`);
    if (serverProcess && !serverProcess.killed) {
      serverProcess.on('exit', () => {
        logger.info('Server stopped');
        resolve();
      });
      serverProcess.kill('SIGTERM');
    } else {
      logger.warn('No server process to stop');
      resolve();
    }
  });
};

// Enable retry on the axios instance
axiosRetry(axios, {
  retries: 5,
  retryDelay: (retryCount) => {
    logger.warn(`Retrying attempt ${retryCount}...`);
    return retryCount * 1000;
  },
  retryCondition: (error) => {
    const status = error.response?.status;
    return status === undefined || status >= 500;
  }
});

const proofServerRequest = async (endpoint: string, payload: Uint8Array): Promise<Uint8Array> => {
  let response;
  try {
    response = await axios.post(`${proofServerUrl}/${endpoint}`, payload.buffer, {
      responseType: 'arraybuffer'
    });
  } catch (e) {
    if (e instanceof Error) {
      logger.warn(`Axios all retries failed: ${e.message}`);
    }
  }
  if (response?.status !== 200) {
    logger.warn(`Request failed: status=${response?.status}, text=${response?.statusText}, data=${response?.data}`);
    throw new Error(response?.statusText);
  }
  return response.data;
};

export const prove = async (
  tx: Transaction<SignatureEnabled, PreProof, PreBinding>
): Promise<Transaction<SignatureEnabled, Proof, PreBinding>> => {
  let proven;
  if (useWasmProving) {
    proven = await tx.prove(wasmProverWorker, CostModel.initialCostModel());
  } else {
    proven = await tx.prove(serverProver, CostModel.initialCostModel());
  }
  // NOTE: If this starts throwing errors... That's not too surprising, this
  // *should* error if transactions have contract calls. Those aren't in the
  // IT-tests today; the only reason this *isn't* behind a `try` block is to
  // make sure this isn't *always* erroring.
  const mockProven = tx.mockProve();
  const bound = proven.bind();
  const errToUndefined = <T>(f: () => T): T | undefined => {
    try {
      return f();
    } catch {
      return undefined;
    }
  };
  const feesMock = errToUndefined(() => mockProven.fees(LedgerParameters.initialParameters()));
  const feesBound = errToUndefined(() => bound.fees(LedgerParameters.initialParameters()));
  // There may be small amounts of drift because the real transaction's
  // binding commitment might be a byte smaller depending on proof
  // randomization.
  const allowedDrift = 200000000000n;
  if (
    feesMock !== undefined &&
    feesBound !== undefined &&
    (feesMock < feesBound || feesBound + allowedDrift < feesMock)
  ) {
    throw new Error(`Mock fee computation didn't match! (mock: ${feesMock}, real: ${feesBound})`);
  }
  return proven;
};

export const startProofServer = () => {
  return startProofServerBinary();
};

export const stopProofServer = () => {
  return stopProofServerBinary();
};

// NOTE: currently assumes that we never need to supply a key :/
const serverProver = {
  check: async (serializedPreimage: Uint8Array, keyLocation: string): Promise<(bigint | undefined)[]> => {
    const payload = createCheckPayload(serializedPreimage);
    const result = await proofServerRequest('check', payload);
    return parseCheckResult(result);
  },
  prove: async (
    serializedPreimage: Uint8Array,
    keyLocation: string,
    overwriteBindingInput?: bigint
  ): Promise<Uint8Array> => {
    const payload = createProvingPayload(serializedPreimage, overwriteBindingInput);
    return proofServerRequest('prove', payload);
  },
  lookupKey: async (keyLocation: string): Promise<ProvingKeyMaterial | undefined> => undefined
};

const callProverWorker = (op: 'check' | 'prove', args: any[]): Promise<any> => {
  return new Promise((resolve, reject) => {
    const worker = new Worker('./dist/src/proof-worker.js', { workerData: [op, __filename, args] });
    worker.on('message', resolve);
    worker.on('error', reject);
    worker.on('exit', (code: number) => {
      if (code !== 0) {
        reject(new Error(`Prover worker stopped with exit code ${code}`));
      }
    });
  });
};

const wasmProverWorker = {
  check: (serializedPreimage: Uint8Array, keyLocation: string): Promise<(bigint | undefined)[]> => {
    return callProverWorker('check', [serializedPreimage, keyLocation]);
  },
  prove: async (
    serializedPreimage: Uint8Array,
    keyLocation: string,
    overwriteBindingInput?: bigint
  ): Promise<Uint8Array> => {
    const t0 = Date.now();
    console.log('started individual proof');
    const result = await callProverWorker('prove', [serializedPreimage, keyLocation, overwriteBindingInput]);
    const tn = Date.now();
    console.log(`finished individual proof in ${(tn - t0) / 1000}s`);
    return result;
  },
  lookupKey: async (keyLocation: string): Promise<ProvingKeyMaterial | undefined> => undefined
};
