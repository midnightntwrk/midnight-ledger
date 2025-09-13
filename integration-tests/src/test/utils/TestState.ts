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

import {
  addressFromKey,
  type Bindingish,
  type BlockContext,
  ClaimRewardsTransaction,
  DustActions,
  DustLocalState,
  type DustOutput,
  DustParameters,
  type DustPublicKey,
  type DustSecretKey,
  Intent,
  LedgerParameters,
  LedgerState,
  type Proofish,
  sampleDustSecretKey,
  sampleSigningKey,
  SignatureErased,
  signatureVerifyingKey,
  type Signaturish,
  type SigningKey,
  Transaction,
  TransactionContext,
  type TransactionResult,
  updatedValue,
  type UserAddress,
  type Utxo,
  WellFormedStrictness,
  ZswapLocalState,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import {
  BALANCING_OVERHEAD,
  DUST_GRACE_PERIOD_IN_SECONDS,
  GENERATION_DECAY_RATE,
  initialParameters,
  LOCAL_TEST_NETWORK_ID,
  NIGHT_DUST_RATIO,
  Static
} from '@/test-objects';

class DustKey {
  readonly secretKey: DustSecretKey;

  constructor(secretKey: DustSecretKey) {
    this.secretKey = secretKey;
  }
  publicKey(): DustPublicKey {
    return this.secretKey.publicKey;
  }
}

class NightKey {
  signingKey: SigningKey;
  constructor(signingKey: SigningKey) {
    this.signingKey = signingKey;
  }
  verifyingKey() {
    return signatureVerifyingKey(this.signingKey);
  }
}

export class TestState {
  initialNightAddress: UserAddress;
  ledger: LedgerState;
  zswap: ZswapLocalState;
  utxos: Set<Utxo>;
  dust: DustLocalState;
  events: Event[];
  time: Date;
  zswapKeys: ZswapSecretKeys;
  nightKey: NightKey;
  dustKey: DustKey;

  private constructor(
    initialNightAddress: UserAddress,
    ledger: LedgerState,
    zswap: ZswapLocalState,
    utxos: Set<Utxo>,
    dust: DustLocalState,
    events: Event[],
    time: Date,
    zswapKeys: ZswapSecretKeys,
    nightKey: NightKey,
    dustKey: DustKey
  ) {
    this.initialNightAddress = initialNightAddress;
    this.ledger = ledger;
    this.zswap = zswap;
    this.utxos = utxos;
    this.dust = dust;
    this.events = events;
    this.time = time;
    this.zswapKeys = zswapKeys;
    this.nightKey = nightKey;
    this.dustKey = dustKey;
  }

  static new(): TestState {
    const nightKey = new NightKey(sampleSigningKey());
    const dustKey = new DustKey(sampleDustSecretKey());
    const initialNightAddress: UserAddress = addressFromKey(nightKey.verifyingKey());

    return new TestState(
      initialNightAddress,
      LedgerState.blank(LOCAL_TEST_NETWORK_ID),
      new ZswapLocalState(),
      new Set(),
      new DustLocalState(new DustParameters(NIGHT_DUST_RATIO, GENERATION_DECAY_RATE, DUST_GRACE_PERIOD_IN_SECONDS)),
      [],
      new Date(0),
      ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1)),
      nightKey,
      dustKey
    );
  }

  context() {
    const block: BlockContext = {
      secondsSinceEpoch: 0n,
      secondsSinceEpochErr: 0,
      parentBlockHash: Buffer.from(new Uint8Array(32)).toString('hex')
    };

    block.secondsSinceEpoch = BigInt(Math.floor(this.time.getTime() / 1000));
    return new TransactionContext(this.ledger, block);
  }

  assertApply(tx: Transaction<Signaturish, Proofish, Bindingish>, strictness: WellFormedStrictness) {
    const result = this.apply(tx, strictness);
    expect(result.type, `result type was: ${result.type}, and error: ${result.error}`).toEqual('success');
  }

  fastForward(dur: bigint) {
    const currSeconds = BigInt(Math.floor(this.time.getTime() / 1000)) + dur;
    const ttl = new Date(Number(currSeconds) * 1000);
    this.time = ttl;

    this.ledger = this.ledger.postBlockUpdate(ttl);
    this.dust = this.dust.processTtls(ttl);
  }

  step() {
    const tenSeconds = 10n;
    this.fastForward(tenSeconds);
  }

  apply(tx: Transaction<Signaturish, Proofish, Bindingish>, strictness: WellFormedStrictness): TransactionResult {
    const context = this.context();
    const vtx = tx.wellFormed(this.ledger, strictness, this.time);
    const [newSt, result] = this.ledger.apply(vtx, context);
    this.ledger = newSt;
    this.zswap = this.zswap.replayEvents(this.zswapKeys, result.events);
    this.dust = this.dust.replayEvents(this.dustKey.secretKey, result.events);
    const pk: UserAddress = addressFromKey(this.nightKey.verifyingKey());
    this.utxos = new Set(
      Array.from(this.ledger.utxo.utxos)
        .map((utxo) => structuredClone(utxo))
        .filter((utxo) => utxo.owner === pk)
    );
    this.step();
    return result;
  }

  applySystemTx(tx: Transaction<Signaturish, Proofish, Bindingish>) {
    const [res, events] = this.ledger.applySystemTx(tx, this.time);
    this.ledger = res;
    this.zswap = this.zswap.replayEvents(this.zswapKeys, events);
    this.dust = this.dust.replayEvents(this.dustKey.secretKey, events);
    const pk: UserAddress = addressFromKey(this.nightKey.verifyingKey());
    this.utxos = new Set(
      Array.from(this.ledger.utxo.utxos)
        .map((utxo) => structuredClone(utxo))
        .filter((utxo) => utxo.owner === pk)
    );
    this.step();
  }

  rewardNight(amount: bigint) {
    this.ledger = this.ledger.testingDistributeNight(this.initialNightAddress, amount, this.time);
    const claimRewardsTransaction = new ClaimRewardsTransaction(
      SignatureMarker.signatureErased,
      LOCAL_TEST_NETWORK_ID,
      amount,
      this.nightKey.verifyingKey(),
      Static.nonce(),
      new SignatureErased()
    );
    const tx = Transaction.fromRewards(claimRewardsTransaction);
    this.apply(tx, new WellFormedStrictness());
  }

  balanceTx(tx: Transaction<Signaturish, Proofish, Bindingish>): Transaction<Signaturish, Proofish, Bindingish> {
    // TODO: add balancing of zswap, which is not needed now
    const guaranteesSegment = 0;
    const fees = tx.fees(LedgerParameters.initialParameters()) + BALANCING_OVERHEAD;
    const balance = tx.imbalances(guaranteesSegment, fees);

    if (Array.from(balance.keys()).some((tt) => tt.tag === 'dust')) {
      let dust = [...balance.entries()]
        .map(([tt, bal]) => (tt.tag === 'dust' && bal < 0 ? -bal : undefined))
        .find((v) => v !== undefined);

      if (dust !== undefined) {
        console.log(
          `balancing ${dust} Dust atomic units / wallet balance: ${this.dust.walletBalance(this.time)} Dust atomic units`
        );

        const oldDust = this.dust;
        const spends = [];

        for (let i = 0; i < oldDust.utxos.length; i++) {
          const qdo = oldDust.utxos[i];
          if (dust === 0n) {
            break;
          }
          const genInfo = oldDust.generationInfo(qdo)!;
          const dustOutput: DustOutput = {
            initialValue: qdo.initialValue,
            owner: qdo.owner,
            nonce: qdo.nonce,
            seq: qdo.seq,
            ctime: qdo.ctime,
            backingNight: qdo.backingNight
          };

          const value: bigint = updatedValue(
            dustOutput.ctime,
            dustOutput.initialValue,
            genInfo,
            this.time,
            initialParameters
          );

          const vFee = value < dust ? value : dust;

          dust = dust - value < 0n ? 0n : dust - value; // dust.saturating_sub(value)
          const [newDust, spend] = this.dust.spend(this.dustKey.secretKey, qdo, vFee, this.time);
          this.dust = newDust;
          spends.push(spend);
        }

        const intent = Intent.new(this.time);
        intent.dustActions = new DustActions(SignatureMarker.signature, ProofMarker.preProof, this.time, spends, []);
        intent.signatureData(0xfeed);
        const tx2Unproven = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
        return tx.merge(tx2Unproven);
      }
    }
    throw new Error('Cannot balance transaction as there were no Dust');
  }
}
