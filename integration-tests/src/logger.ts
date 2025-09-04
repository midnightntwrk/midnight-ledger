// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

import pinoPretty from 'pino-pretty';
import pino from 'pino';

const level = 'info' as const;

export const createLogger = async (logPath: string): Promise<pino.Logger> => {
  const pretty: pinoPretty.PrettyStream = pinoPretty({
    colorize: true,
    sync: true
  });
  const prettyFile: pinoPretty.PrettyStream = pinoPretty({
    colorize: false,
    sync: true,
    append: true,
    mkdir: true,
    destination: logPath
  });
  return pino(
    {
      level,
      depthLimit: 20
    },
    pino.multistream([
      { stream: pretty, level },
      { stream: prettyFile, level }
    ])
  );
};

export const createDefaultTestLogger = async () => {
  return createLogger(`logs/tests.log`);
};
