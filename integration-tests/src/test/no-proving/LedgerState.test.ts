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
  type AlignedValue,
  communicationCommitmentRandomness,
  ChargedState,
  ContractCallPrototype,
  ContractDeploy,
  ContractOperation,
  ContractState,
  LedgerState,
  nativeToken,
  StateMap,
  StateValue,
  Transaction,
  ZswapChainState,
  Intent,
  TransactionContext,
  WellFormedStrictness,
  feeToken,
  unshieldedToken,
  shieldedToken,
  DustState
} from '@midnight-ntwrk/ledger';

import { ONE_KB, Random, Static, TestResource, VERSION_HEADER } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - LedgerState', () => {
  const GLOBAL_TTL = 3600;

  /**
   * Test serialization and deserialization of ledger state.
   *
   * @given A LedgerState with ZswapChainState
   * @when Serializing and then deserializing
   * @then Should maintain object integrity and string representation
   */
  test('should deserialize serialized state', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const serialized = ledgerState.serialize();
    const deserialized = LedgerState.deserialize(serialized);

    expect(deserialized.toString()).toEqual(ledgerState.toString());
  });

  /**
   * Test state consistency after serialization round-trip.
   *
   * @given A LedgerState with ZswapChainState
   * @when Serializing and deserializing the state
   * @then Should maintain all state properties including firstFree and blockRewardPool
   */
  test('should not differ after serialization', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const serialized = ledgerState.serialize();
    const ledgerStateDeserialized = LedgerState.deserialize(serialized);

    expect(ledgerStateDeserialized.zswap.firstFree).toEqual(ledgerState.zswap.firstFree);
    expect(ledgerStateDeserialized.blockRewardPool).toEqual(ledgerState.blockRewardPool);
    expect(ledgerStateDeserialized.toString(true)).toEqual(ledgerState.toString(true));
  });

  /**
   * Test contract state retrieval for empty contract address.
   *
   * @given A LedgerState and a random contract address
   * @when Indexing the contract address
   * @then Should return undefined for non-existent contract
   */
  test('should get undefined contract state at empty contract address', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);

    expect(ledgerState.index(Random.contractAddress())).toBeUndefined();
    assertSerializationSuccess(ledgerState);
  });

  /**
   * Test error handling for invalid contract addresses.
   *
   * @given A LedgerState and various invalid contract address formats
   * @when Attempting to index with invalid address
   * @then Should throw appropriate validation errors
   */
  it.each([
    ['should throw error on 0 length contract address', 'failed to fill whole buffer', ''],
    ['should throw error on blank contract address', 'Odd number of digits', ' '],
    ['should throw error on long contract address', "Invalid character 'z' at position 16", 'abcdef0123456789zz'],
    [
      'should throw error on blank contract address',
      'Odd number of digits',
      '0200ab975be1b3a2d90dd9fc3ebf1a46abaecc542648c70a62e42dd300d38cd4ab9b '
    ],
    [
      'should throw error on a too short contract address',
      'Not all bytes read, 1 bytes remaining',
      `${VERSION_HEADER + '1'.repeat(62)}`
    ]
  ])('%s(expected error:"%s")', (_, expectedError, contractAddress) => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);

    expect(() => ledgerState.index(contractAddress)).toThrow(expectedError);
  });

  /**
   * Test creation of blank ledger state.
   *
   * @given No parameters
   * @when Creating a blank LedgerState
   * @then Should initialize with default values and proper native, fee, shielded, and unshielded token supply
   */
  test('should create blank ledger state', () => {
    const ledgerState = LedgerState.blank('local-test');

    expect(ledgerState.dust.toString()).toEqual(new DustState().toString());
    expect(ledgerState.reservePool).toEqual(24000000000000000n);
    expect(ledgerState.blockRewardPool).toEqual(0n);

    expect(ledgerState.treasuryBalance(nativeToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(feeToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(shieldedToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(unshieldedToken())).toEqual(0n);

    assertSerializationSuccess(ledgerState);
  });

  /**
   * Test successful creation of LedgerState.
   *
   * @given A ZswapChainState
   * @when Creating a new LedgerState
   * @then Should initialize with correct default values and string representation
   */
  test('should create successfully', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);

    expect(ledgerState.reservePool).toEqual(24000000000000000n);
    expect(ledgerState.blockRewardPool).toEqual(0n);

    expect(ledgerState.treasuryBalance(nativeToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(feeToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(shieldedToken())).toEqual(0n);
    expect(ledgerState.treasuryBalance(unshieldedToken())).toEqual(0n);

    expect(ledgerState.toString()).toMatch(/LedgerState \{.*/);
    assertSerializationSuccess(ledgerState);
  });

  /**
   * Test updating contract state in ledger.
   *
   * @given A LedgerState and contract address with state map
   * @when Updating the contract index with new state
   * @then Should update contract state and maintain state map integrity
   */
  test('should update contract state', () => {
    const contractAddress = Random.contractAddress();
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    const stateValue = new ChargedState(StateValue.newMap(stateMap));
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const contractState = ledgerState.index(contractAddress);
    const updatedLedgerState = ledgerState.updateIndex(contractAddress, stateValue, new Map());

    expect(updatedLedgerState.index(contractAddress)?.data).not.toEqual(contractState?.data);
    expect(updatedLedgerState.index(contractAddress)?.data.state.asMap()?.keys()).toEqual(
      stateValue.state.asMap()?.keys()
    );
    expect(updatedLedgerState.index(contractAddress)?.data.state.asMap()?.get(Static.alignedValue)?.toString()).toEqual(
      StateValue.newNull().toString()
    );
    assertSerializationSuccess(updatedLedgerState);
  });

  /**
   * Test handling of empty state map.
   *
   * @given A LedgerState and empty StateMap
   * @when Updating contract index with empty state
   * @then Should create contract with empty state map
   */
  test('should handle empty state map', () => {
    const contractAddress = Random.contractAddress();
    const stateMap = new StateMap();
    const stateValue = new ChargedState(StateValue.newMap(stateMap));
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const updatedLedgerState = ledgerState.updateIndex(contractAddress, stateValue, new Map());

    expect(updatedLedgerState.index(contractAddress)?.data.state.asMap()?.keys().length).toEqual(0);
    assertSerializationSuccess(updatedLedgerState);
  });

  /**
   * Test handling of non-empty state map.
   *
   * @given A LedgerState and StateMap with one entry
   * @when Updating contract index with the state
   * @then Should create contract with correct state map entry
   */
  test('should handle non-empty state map', () => {
    const contractAddress = Random.contractAddress();
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    const stateValue = new ChargedState(StateValue.newMap(stateMap));
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const updatedLedgerState = ledgerState.updateIndex(contractAddress, stateValue, new Map());

    expect(updatedLedgerState.index(contractAddress)?.data.state.asMap()?.keys().length).toEqual(1);
    expect(updatedLedgerState.index(contractAddress)?.data.state.asMap()?.get(Static.alignedValue)?.toString()).toEqual(
      StateValue.newNull().toString()
    );
    assertSerializationSuccess(updatedLedgerState);
  });

  /**
   * Test multiple updates to contract state.
   *
   * @given A LedgerState with an initial contract state update
   * @when Applying a second update to the same contract
   * @then Should properly update the contract state maintaining integrity
   */
  test('should handle multiple updates to contract state', () => {
    const contractAddress = Random.contractAddress();
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    const stateValue = new ChargedState(StateValue.newMap(stateMap));
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const updatedLedgerState = ledgerState.updateIndex(contractAddress, stateValue, new Map());

    let newStateMap = new StateMap();
    newStateMap = newStateMap.insert(Static.alignedValue, StateValue.newNull());
    const newStateValue = new ChargedState(StateValue.newMap(newStateMap));
    const newUpdatedLedgerState = updatedLedgerState.updateIndex(contractAddress, newStateValue, new Map());

    expect(newUpdatedLedgerState.index(contractAddress)?.data.state.asMap()?.keys().length).toEqual(1);
    expect(
      newUpdatedLedgerState.index(contractAddress)?.data.state.asMap()?.get(Static.alignedValue)?.toString()
    ).toEqual(StateValue.newNull().toString());
    assertSerializationSuccess(newUpdatedLedgerState);
  });

  /**
   * Test inserting map with large key and serialization.
   *
   * @given A LedgerState and StateMap with large key (64KB)
   * @when Updating contract index and serializing/deserializing
   * @then Should handle large keys correctly through serialization round-trip
   */
  test('should insert map with big key and serialize and deserialize ledger state', () => {
    const contractAddress = Random.contractAddress();
    let stateMap = new StateMap();
    const alignedValue: AlignedValue = {
      value: [new Uint8Array(64 * ONE_KB).fill(255)],
      alignment: [
        {
          tag: 'atom',
          value: { tag: 'bytes', length: 64 * ONE_KB }
        }
      ]
    };
    stateMap = stateMap.insert(alignedValue, StateValue.newNull());
    const stateValue = new ChargedState(StateValue.newMap(stateMap));
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const updatedLedgerState = ledgerState.updateIndex(contractAddress, stateValue, new Map());
    const serialized = updatedLedgerState.serialize();
    const deserialized = LedgerState.deserialize(serialized);

    expect(deserialized.toString()).toEqual(updatedLedgerState.toString());
  });

  /**
   * Test applying deploy and call contract transactions.
   *
   * @given A LedgerState and two transactions (deploy then call)
   * @when Applying both transactions sequentially
   * @then Should successfully execute both transactions and update state correctly
   */
  test('should apply 2 transactions - deploy and call contract', () => {
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractOperation.verifierKey = TestResource.operationVerifierKey();
    contractState.setOperation('testOperation', contractOperation);
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(Static.calcBlockTime(new Date(0), 50)).addDeploy(contractDeploy);
    const unprovenOfferGuaranteed = Random.unprovenOfferFromOutput();
    const unprovenTransaction = Transaction.fromParts('local-test', unprovenOfferGuaranteed, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    strictness.verifyContractProofs = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [ledgerStateAfter, txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set([contractDeploy.address]))
    );

    expect(txResult.error).toBeUndefined();
    expect(txResult.type).toEqual('success');
    expect(ledgerStateAfter.zswap.firstFree.toString()).toEqual('1');

    const contractCallPrototype = new ContractCallPrototype(
      contractDeploy.address,
      'testOperation',
      contractOperation,
      {
        gas: {
          readTime: 0n,
          computeTime: 10000000000n,
          bytesWritten: 0n,
          bytesDeleted: 0n
        },
        effects: {
          claimedNullifiers: [],
          claimedShieldedReceives: [],
          claimedShieldedSpends: [],
          claimedContractCalls: [],
          shieldedMints: new Map(),
          unshieldedMints: new Map(),
          unshieldedInputs: new Map(),
          unshieldedOutputs: new Map(),
          claimedUnshieldedSpends: new Map()
        },
        program: [{ noop: { n: 5 } }]
      },
      undefined,
      [Static.alignedValue],
      Static.alignedValue,
      Static.alignedValue,
      communicationCommitmentRandomness(),
      'key_location'
    );
    const intent2 = Intent.new(Static.calcBlockTime(new Date(0), 50)).addCall(contractCallPrototype);

    const unprovenOfferGuaranteed2 = Random.unprovenOfferFromOutput();
    const unprovenTransaction2 = Transaction.fromParts('local-test', unprovenOfferGuaranteed2, undefined, intent2);
    const proofErasedTransaction2 = unprovenTransaction2.eraseProofs();
    const verifiedTransaction2 = proofErasedTransaction2.wellFormed(ledgerStateAfter, strictness, new Date(0));
    const [ledgerStateAfter2, txResult2] = ledgerStateAfter.apply(
      verifiedTransaction2,
      new TransactionContext(ledgerStateAfter, blockContext, new Set([contractDeploy.address]))
    );

    expect(txResult2.type).toEqual('success');
    expect(ledgerStateAfter2.zswap.firstFree.toString()).toEqual('2');
    expect(ledgerStateAfter2.zswap.toString()).not.toEqual(ledgerStateAfter.zswap.toString());
    assertSerializationSuccess(ledgerStateAfter);
    assertSerializationSuccess(ledgerStateAfter2);
  });

  /**
   * Test replay attack protection.
   *
   * @given A LedgerState and the same deploy transaction applied twice
   * @when Applying the same transaction to prevent replay attack
   * @then Should succeed first time and fail second time with replay protection error
   */
  test('should apply 2 transactions - apply same deploy tx twice - replay attack', () => {
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractOperation.verifierKey = TestResource.operationVerifierKey();
    contractState.setOperation('testOperation', contractOperation);
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(new Date(Date.now())).addDeploy(contractDeploy);
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());

    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(Date.now() - 5_000)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date());
    const [ledgerStateAfter, txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set([contractDeploy.address]))
    );
    const [ledgerStateAfter2, txResult2] = ledgerStateAfter.apply(
      verifiedTransaction,
      new TransactionContext(ledgerStateAfter, blockContext, new Set([contractDeploy.address]))
    );

    expect(txResult.type).toEqual('success');
    expect(txResult2.type).toEqual('failure');
    expect(txResult2.error).toEqual('replay protection has been violated: IntentAlreadyExists');
    expect(ledgerStateAfter2.zswap.firstFree.toString()).toEqual('0');
    expect(ledgerStateAfter.zswap.firstFree.toString()).toEqual('0');
  });

  /**
   * Test transaction with expired TTL.
   *
   * @given A LedgerState and transaction with TTL in the past
   * @when Applying the transaction
   * @then Should fail with IntentTtlExpired error
   */
  test('should apply transaction with ttl in past', () => {
    const now = new Date();
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractOperation.verifierKey = TestResource.operationVerifierKey();
    contractState.setOperation('testOperation', contractOperation);
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(Static.calcBlockTime(now, -1)).addDeploy(contractDeploy);
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(now),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(
      ledgerState,
      strictness,
      Static.calcBlockTime(new Date(), -50)
    );
    const [ledgerStateAfter, txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set([contractDeploy.address]))
    );

    expect(txResult.error).toMatch('replay protection has been violated: IntentTtlExpired');
    expect(txResult.type).toEqual('failure');
    expect(ledgerStateAfter.zswap.firstFree.toString()).toEqual('0');
  });

  /**
   * Test transaction with TTL too far in future.
   *
   * @given A LedgerState and transaction with TTL beyond global TTL limit
   * @when Applying the transaction
   * @then Should fail with IntentTtlTooFarInFuture error
   */
  test('should apply transaction with ttl too far away in future', () => {
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractOperation.verifierKey = TestResource.operationVerifierKey();
    contractState.setOperation('testOperation', contractOperation);
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(new Date((GLOBAL_TTL + 1) * 1000)).addDeploy(contractDeploy);
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(
      ledgerState,
      strictness,
      new Date((GLOBAL_TTL + 1) * 1000)
    );
    const [ledgerStateAfter, txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set([contractDeploy.address]))
    );

    expect(txResult.error).toMatch('replay protection has been violated: IntentTtlTooFarInFuture');
    expect(txResult.type).toEqual('failure');
    expect(ledgerStateAfter.zswap.firstFree.toString()).toEqual('0');
  });

  /**
   * Test unclaimed block rewards functionality.
   *
   * @given A blank LedgerState
   * @when Querying unclaimed block rewards for the user
   * @then Should return 0 for unclaimed block rewards
   */
  test('should return bigint for unclaimed block rewards', () => {
    const ledgerState = LedgerState.blank('local-test');
    const userAddress = Random.userAddress();

    const unclaimedAmount = ledgerState.unclaimedBlockRewards(userAddress);

    expect(unclaimedAmount).toEqual(0n);
  });
});
