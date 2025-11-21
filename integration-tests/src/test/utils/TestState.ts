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
  createShieldedCoinInfo,
  DustActions,
  DustLocalState,
  type DustOutput,
  DustParameters,
  type DustPublicKey,
  DustRegistration,
  type DustSecretKey,
  Intent,
  LedgerParameters,
  LedgerState,
  type PreProof,
  type Proofish,
  type QualifiedShieldedCoinInfo,
  sampleDustSecretKey,
  sampleSigningKey,
  type ShieldedCoinInfo,
  type SignatureEnabled,
  SignatureErased,
  signatureVerifyingKey,
  type Signaturish,
  type SigningKey,
  type SyntheticCost,
  type TokenType,
  Transaction,
  TransactionContext,
  type TransactionResult,
  UnshieldedOffer,
  updatedValue,
  type UserAddress,
  type Utxo,
  type UtxoOutput,
  WellFormedStrictness,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import {
  DEFAULT_TOKEN_TYPE,
  DUST_GRACE_PERIOD_IN_SECONDS,
  GENERATION_DECAY_RATE,
  initialParameters,
  LOCAL_TEST_NETWORK_ID,
  NIGHT_DUST_RATIO,
  Random,
  type ShieldedTokenType,
  type UnshieldedTokenType
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
      ZswapSecretKeys.fromSeed(Random.generate32Bytes()),
      nightKey,
      dustKey
    );
  }

  spendDust(limit: number) {
    if (this.dust.utxos.length < limit) {
      throw new Error('Not enough DUST utxos to spend');
    }
    const spendsBlockBefore = [];

    for (let i = 0; i < limit; i++) {
      const qdo = this.dust.utxos[0];
      const genInfo = this.dust.generationInfo(qdo)!;
      const dustOutput: DustOutput = {
        initialValue: qdo.initialValue,
        owner: qdo.owner,
        nonce: qdo.nonce,
        seq: qdo.seq,
        ctime: qdo.ctime,
        backingNight: qdo.backingNight
      };

      const vFee = updatedValue(dustOutput.ctime, dustOutput.initialValue, genInfo, this.time, initialParameters);
      const [newDust, spend] = this.dust.spend(this.dustKey.secretKey, qdo, vFee, this.time);
      this.dust = newDust;
      spendsBlockBefore.push(spend);
    }

    const intent = Intent.new(this.time);
    intent.dustActions = new DustActions<SignatureEnabled, PreProof>(
      SignatureMarker.signature,
      ProofMarker.preProof,
      this.time,
      spendsBlockBefore,
      []
    );
    intent.signatureData(0xfeed);

    const tx = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

    const balancedTx = this.balanceTx(tx.eraseProofs());
    this.assertApply(balancedTx, new WellFormedStrictness(), balancedTx.cost(this.ledger.parameters));
  }

  context() {
    const block: BlockContext = {
      secondsSinceEpoch: BigInt(this.time.getTime() / 1000),
      secondsSinceEpochErr: 0,
      parentBlockHash: Buffer.from(new Uint8Array(32)).toString('hex')
    };
    return new TransactionContext(this.ledger, block);
  }

  assertApply(
    tx: Transaction<Signaturish, Proofish, Bindingish>,
    strictness: WellFormedStrictness,
    blockFullness?: SyntheticCost
  ) {
    const result = this.apply(tx, strictness, blockFullness);
    expect(result.type, `result type was: ${result.type}, and error: ${result.error}`).toEqual('success');
  }

  fastForward(dur: bigint, blockFullness?: SyntheticCost) {
    const currSeconds = BigInt(Math.floor(this.time.getTime() / 1000)) + dur;
    const ttl = new Date(Number(currSeconds) * 1000);
    this.time = ttl;

    this.ledger = this.ledger.postBlockUpdate(ttl, blockFullness);
    this.dust = this.dust.processTtls(ttl);
  }

  step(blockFullness?: SyntheticCost) {
    const tenSeconds = 10n;
    this.fastForward(tenSeconds, blockFullness);
  }

  apply(
    tx: Transaction<Signaturish, Proofish, Bindingish>,
    strictness: WellFormedStrictness,
    blockFullness?: SyntheticCost
  ): TransactionResult {
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
    this.step(blockFullness);
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

  giveFeeToken(utxos: number, amount: bigint) {
    this.dustGenerationRegister();
    for (let i = 0; i < utxos; i++) {
      this.rewardNight(amount);
    }
    this.fastForward(initialParameters.timeToCapSeconds);
  }

  dustGenerationRegister() {
    const reg = new DustRegistration<SignatureEnabled>(
      SignatureMarker.signature,
      this.nightKey.verifyingKey(),
      this.dustKey.publicKey(),
      0n
    );
    const actions: DustActions<SignatureEnabled, PreProof> = new DustActions<SignatureEnabled, PreProof>(
      SignatureMarker.signature,
      ProofMarker.preProof,
      this.time,
      [],
      [reg]
    );
    const intent = Intent.new(this.time);
    intent.dustActions = actions;
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    strictness.verifySignatures = false;
    this.assertApply(tx, strictness);
  }

  rewardNight(amount: bigint) {
    this.ledger = this.ledger.testingDistributeNight(this.initialNightAddress, amount, this.time);
    const claimRewardsTransaction = new ClaimRewardsTransaction(
      SignatureMarker.signatureErased,
      LOCAL_TEST_NETWORK_ID,
      amount,
      this.nightKey.verifyingKey(),
      Random.nonce(),
      new SignatureErased()
    );
    const tx = Transaction.fromRewards(claimRewardsTransaction);
    this.assertApply(tx, new WellFormedStrictness());
  }

  rewardsShielded(token: ShieldedTokenType, amount: bigint) {
    const coin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, amount);
    const output = ZswapOutput.new(coin, 0, this.zswapKeys.coinPublicKey, this.zswapKeys.encryptionPublicKey);
    const offer = ZswapOffer.fromOutput(output, token.raw, amount);
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    this.assertApply(tx, strictness);
  }

  rewardsUnshielded(token: UnshieldedTokenType, amount: bigint) {
    if (token.raw === DEFAULT_TOKEN_TYPE) {
      this.rewardNight(amount);
      return;
    }
    const utxo: UtxoOutput = {
      owner: addressFromKey(this.nightKey.verifyingKey()),
      type: token.raw,
      value: amount
    };

    const offer = UnshieldedOffer.new([], [utxo], []);
    const intent = Intent.new(this.time);
    intent.guaranteedUnshieldedOffer = offer;
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    this.assertApply(tx, strictness);
  }

  static getDustImbalance(mergedTx: Transaction<Signaturish, Proofish, Bindingish>): bigint | undefined {
    const guaranteesSegment = 0;
    const fees = mergedTx.fees(LedgerParameters.initialParameters());
    const imbalances = mergedTx.imbalances(guaranteesSegment, fees);
    if (!imbalances) return undefined;
    const dustImbalance = Array.from(imbalances.entries()).find(([tt, bal]) => tt.tag === 'dust' && bal < 0n);

    return dustImbalance ? -dustImbalance[1] : undefined;
  }

  balanceTx(txi: Transaction<Signaturish, Proofish, Bindingish>): Transaction<Signaturish, Proofish, Bindingish> {
    let tx = txi;
    const fees = undefined;
    const zswapToBalance: Transaction<Signaturish, Proofish, Bindingish>[] = [];
    [0, 1].forEach((segmentId) => {
      let imbalance;
      try {
        imbalance = tx.imbalances(segmentId, fees);
      } catch (err: unknown) {
        if (err instanceof Error && err.message.includes("segment doesn't exist")) {
          return;
        }
        throw err;
      }

      if (imbalance.size === 0) return;
      (imbalance as Map<TokenType, bigint>).forEach((val, tt) => {
        if (tt.tag === 'dust') {
          return;
        }
        const target = -val;
        let totalInp = 0n;
        const inputCoins: QualifiedShieldedCoinInfo[] = [];

        this.zswap.coins.forEach((coin) => {
          if (totalInp >= target) return;
          if (coin.type !== tt.raw) return;
          inputCoins.push(coin);
          totalInp += coin.value;
        });

        const outputVal = totalInp >= -val ? totalInp - -val : 0n;

        const inputs = inputCoins.map((coin) => {
          const [nextState, inp] = this.zswap.spend(this.zswapKeys, coin, segmentId);
          this.zswap = nextState;
          return inp;
        });

        const outputCoin = createShieldedCoinInfo(tt.raw, outputVal);
        const output = ZswapOutput.new(
          outputCoin,
          segmentId,
          this.zswapKeys.coinPublicKey,
          this.zswapKeys.encryptionPublicKey
        );

        let offer;
        const delta = totalInp - outputVal;
        for (let i = 0; i < inputs.length; i++) {
          const input = inputs[i];
          if (i === 0) {
            offer = ZswapOffer.fromInput(input, tt.raw, delta);
          } else {
            const offerFromInput = ZswapOffer.fromInput(input, tt.raw, delta);
            offer = offer!.merge(offerFromInput);
          }
        }
        if (outputVal > 0n) {
          const offerFromOutput = ZswapOffer.fromOutput(output, tt.raw, 0n);
          offer = offer!.merge(offerFromOutput);
        }

        if (segmentId === 0) {
          zswapToBalance.push(Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer));
        } else {
          zswapToBalance.push(Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, offer));
        }
      });

      zswapToBalance.forEach((txb) => {
        tx = tx.merge(txb.eraseProofs());
      });
    });

    let mergedTx = tx;
    let unprovenBal = null;
    const oldDust = this.dust;
    let lastDust = 0n;

    let dust = TestState.getDustImbalance(mergedTx);
    while (dust !== undefined) {
      dust += lastDust;
      lastDust = dust;
      console.log(
        `balancing ${dust} Dust atomic units / wallet balance: ${this.dust.walletBalance(this.time)} Dust atomic units`
      );

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

        console.log(`adding utxo of ${vFee} Dust atomic units`);
        dust = dust - value < 0n ? 0n : dust - value; // dust.saturating_sub(value)
        const [newDust, spend] = this.dust.spend(this.dustKey.secretKey, qdo, vFee, this.time);
        this.dust = newDust;
        spends.push(spend);
      }

      if (dust > 0n) {
        throw new Error("failed to balance testing transaction's dust");
      }

      const intent = Intent.new(this.time);
      intent.dustActions = new DustActions(SignatureMarker.signature, ProofMarker.preProof, this.time, spends, []);
      intent.signatureData(0xfeed);

      const tx2Unproven = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
      mergedTx = tx.merge(tx2Unproven.eraseProofs());
      unprovenBal = tx2Unproven;

      dust = TestState.getDustImbalance(mergedTx);
    }
    if (unprovenBal) {
      mergedTx = tx.merge(unprovenBal.eraseProofs());
    }
    return mergedTx;
  }
}
