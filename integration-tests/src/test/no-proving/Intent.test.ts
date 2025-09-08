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
  communicationCommitmentRandomness,
  ContractCallPrototype,
  ContractDeploy,
  ContractOperation,
  ContractOperationVersion,
  ContractState,
  Intent,
  MaintenanceUpdate,
  type PreBinding,
  sampleIntentHash,
  sampleUserAddress,
  UnshieldedOffer,
  VerifierKeyRemove
} from '@midnight-ntwrk/ledger';
import { getNewUnshieldedOffer, Random, Static } from '@/test-objects';

describe('Ledger API - Intent', () => {
  const TTL = new Date();
  TTL.setMilliseconds(0);

  /**
   * Test creation of new intent with default values.
   *
   * @given A TTL date
   * @when Creating a new Intent
   * @then Should have undefined offers, empty actions array, and proper TTL
   */
  test('should create a new intent with default values', () => {
    const intent = Intent.new(TTL);

    expect(intent.guaranteedUnshieldedOffer).toBeUndefined();
    expect(intent.fallibleUnshieldedOffer).toBeUndefined();
    expect(intent.actions).toEqual([]);
    expect(intent.dustActions).toBeUndefined();

    // BUG: PM-17063
    expect(intent.ttl).toEqual(TTL);
    expect(intent.binding).toBeDefined();
    expect(intent.toString()).toMatch(/Intent .*/);
  });

  /**
   * Test signature data generation for bound intent.
   *
   * @given An intent bound with segment ID 1
   * @when Generating signature data for bound intent
   * @then Should return signature data with positive length and valid binding
   */
  test('should generate valid signature data when bound', () => {
    const intent = Intent.new(TTL);
    const bound = intent.bind(1);
    const signatureData = bound.signatureData(0);

    expect(signatureData.length).toBeGreaterThan(0);
    expect(bound.binding).toBeDefined();
  });

  /**
   * Test error handling when binding with segment 0.
   *
   * @given An unbound intent
   * @when Attempting to bind with segment ID 0
   * @then Should throw 'Segment ID cannot be 0' error
   */
  test('should throw error when bind with segment 0', () => {
    const intent = Intent.new(TTL);
    expect(() => intent.bind(0)).toThrow('Segment ID cannot be 0');
  });

  /**
   * Test error handling when attempting to bind an already bound intent.
   *
   * @given An intent that has already been bound
   * @when Attempting to bind it again with a different segment ID
   * @then Should throw 'Intent cannot be bound.' error
   */
  test('should throw error when attempting to bind twice', () => {
    const intent = Intent.new(TTL);
    const bound = intent.bind(1);
    expect(() => bound.bind(2)).toThrow('Intent cannot be bound.');

    const signatureData = bound.signatureData(0);

    expect(signatureData.length).toBeGreaterThan(0);
    expect(bound.binding).toBeDefined();
  });

  /**
   * Test adding contract call to intent actions.
   *
   * @given A contract call prototype with contract address and operation
   * @when Adding the call to an intent
   * @then Should add one action to the intent with matching string representation
   */
  test('should add contract call to intent actions', () => {
    const commitmentRandomness = communicationCommitmentRandomness();
    const contractAddress = Random.contractAddress();

    const contractCallPrototype = new ContractCallPrototype(
      contractAddress,
      'entry',
      new ContractOperation(),
      undefined,
      undefined,
      [Static.alignedValue],
      Static.alignedValue,
      Static.alignedValue,
      commitmentRandomness,
      'key_location'
    );

    const intent = Intent.new(TTL);
    const updated = intent.addCall(contractCallPrototype);

    expect(updated.actions.length).toEqual(1);
    expect(updated.actions.at(0)?.toString()).toEqual(
      contractCallPrototype.intoCall('pre-binding' as unknown as PreBinding).toString()
    );
  });

  /**
   * Test adding contract deploy to intent actions.
   *
   * @given A contract deploy with contract state
   * @when Adding the deploy to an intent
   * @then Should add one action to the intent with matching string representation
   */
  test('should add contract deploy to intent actions', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    const intent = Intent.new(TTL);
    const updated = intent.addDeploy(contractDeploy);

    expect(updated.actions.length).toEqual(1);
    expect(updated.actions.at(0)?.toString()).toEqual(contractDeploy.toString());
  });

  /**
   * Test adding maintenance update to intent actions.
   *
   * @given A maintenance update with contract address and verifier key operations
   * @when Adding the maintenance update to an intent
   * @then Should add one action to the intent with matching string representation
   */
  test('should add maintenance update to intent actions', () => {
    const maintenanceUpdate = new MaintenanceUpdate(
      Random.contractAddress(),
      [new VerifierKeyRemove('operation', new ContractOperationVersion('v2'))],
      0n
    );

    const intent = Intent.new(TTL);
    const updated = intent.addMaintenanceUpdate(maintenanceUpdate);

    expect(updated.actions.length).toEqual(1);
    expect(updated.actions.at(0)?.toString()).toEqual(maintenanceUpdate.toString());
  });

  /**
   * Test adding multiple different action types to intent.
   *
   * @given A contract call, contract deploy, and maintenance update
   * @when Adding all three actions to an intent sequentially
   * @then Should have 3 actions in the correct order with matching string representations
   */
  test('should add multiple different actions to intent', () => {
    const commitmentRandomness = communicationCommitmentRandomness();
    const contractAddress = Random.contractAddress();

    const contractCallPrototype = new ContractCallPrototype(
      contractAddress,
      'entry',
      new ContractOperation(),
      undefined,
      undefined,
      [Static.alignedValue],
      Static.alignedValue,
      Static.alignedValue,
      commitmentRandomness,
      'key_location'
    );

    // BUG
    // const contractCall = contractCallPrototype.intoCall(new PreBinding());

    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    const maintenanceUpdate = new MaintenanceUpdate(
      Random.contractAddress(),
      [new VerifierKeyRemove('operation', new ContractOperationVersion('v2'))],
      0n
    );

    let intent = Intent.new(TTL);
    intent = intent.addDeploy(contractDeploy);
    intent = intent.addCall(contractCallPrototype);
    intent = intent.addMaintenanceUpdate(maintenanceUpdate);

    expect(intent.actions.length).toEqual(3);
    expect(intent.actions.at(0)?.toString()).toEqual(contractDeploy.toString());
    expect(intent.actions.at(1)?.toString()).toEqual(
      contractCallPrototype.intoCall('pre-binding' as unknown as PreBinding).toString()
    );
    expect(intent.actions.at(2)?.toString()).toEqual(maintenanceUpdate.toString());
  });

  /**
   * Test direct assignment of actions array.
   *
   * @given An intent and a contract deploy action
   * @when Directly assigning the actions array
   * @then Should allow direct assignment and maintain action string representation
   */
  test('should allow direct assignment of actions array', () => {
    const intent = Intent.new(TTL);

    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    intent.actions = [contractDeploy];
    expect(intent.actions.toString()).toEqual(contractDeploy.toString());
  });

  /**
   * Test proof erasure on unproven intent.
   *
   * @given An intent with a contract deploy action
   * @when Erasing proofs from the intent
   * @then Should return intent with identical string representation
   */
  test('should handle erasing proofs on unproven intent', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    const intent = Intent.new(TTL);
    const updated = intent.addDeploy(contractDeploy);
    const erased = updated.eraseProofs();

    expect(erased.toString()).toEqual(updated.toString());
  });

  /**
   * Test setting and getting guaranteed unshielded offer.
   *
   * @given An intent and an unshielded offer
   * @when Setting and getting guaranteedUnshieldedOffer property
   * @then Should properly store and retrieve the offer with matching string representation
   */
  test('should set and get guaranteedUnshieldedOffer', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();
    expect(intent.guaranteedUnshieldedOffer).toBeUndefined();

    intent.guaranteedUnshieldedOffer = offer;

    expect(intent.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test setting and getting fallible unshielded offer.
   *
   * @given An intent and an unshielded offer
   * @when Setting and getting fallibleUnshieldedOffer property
   * @then Should properly store and retrieve the offer with matching string representation
   */
  test('should set and get fallibleUnshieldedOffer', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();
    expect(intent.fallibleUnshieldedOffer).toBeUndefined();

    intent.fallibleUnshieldedOffer = offer;

    expect(intent.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test retention of unshielded offers after adding operations.
   *
   * @given An intent with both guaranteed and fallible unshielded offers
   * @when Adding a contract deploy operation
   * @then Should retain both offers with matching string representations
   */
  test('should retain unshielded offers after adding operations', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const updatedIntent = intent.addDeploy(contractDeploy);

    expect(updatedIntent.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
    expect(updatedIntent.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test retention of unshielded offers after binding.
   *
   * @given An intent with both guaranteed and fallible unshielded offers
   * @when Binding the intent with segment ID 1
   * @then Should retain both offers with matching string representations
   */
  test('should retain unshielded offers after binding', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    const boundIntent = intent.bind(1);

    expect(boundIntent.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
    expect(boundIntent.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test retention of unshielded offers after erasing proofs.
   *
   * @given An intent with both guaranteed and fallible unshielded offers
   * @when Erasing proofs from the intent
   * @then Should retain both offers with matching string representations
   */
  test('should retain unshielded offers after erasing proofs', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    const erasedIntent = intent.eraseProofs();

    expect(erasedIntent.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
    expect(erasedIntent.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test preservation of unshielded offers during serialization and deserialization.
   *
   * @given An intent with both guaranteed and fallible unshielded offers
   * @when Serializing and then deserializing the intent
   * @then Should preserve both offers with identical string representations
   */
  test('should preserve unshielded offers during serialization and deserialization', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    const serialized = intent.serialize();

    const deserialized = Intent.deserialize('signature', 'pre-proof', 'pre-binding', serialized);

    expect(deserialized.guaranteedUnshieldedOffer?.toString()).toBe(offer.toString());
    expect(deserialized.fallibleUnshieldedOffer?.toString()).toBe(offer.toString());
  });

  /**
   * Test handling of setting unshielded offers to null and undefined.
   *
   * @given An intent with unshielded offers already set
   * @when Setting both offers to undefined
   * @then Should properly handle undefined assignment and clear the offers
   */
  test('should handle setting unshielded offers to null and undefined', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    expect(intent.guaranteedUnshieldedOffer.toString()).toBe(offer.toString());
    expect(intent.fallibleUnshieldedOffer.toString()).toBe(offer.toString());

    intent.guaranteedUnshieldedOffer = undefined;
    intent.fallibleUnshieldedOffer = undefined;

    expect(intent.guaranteedUnshieldedOffer).toBeUndefined();
    expect(intent.fallibleUnshieldedOffer).toBeUndefined();
  });

  /**
   * Test preservation of unshielded offers through multiple operations.
   *
   * @given An intent with unshielded offers, deploy, and call actions
   * @when Performing multiple operations and serialization/deserialization
   * @then Should preserve offers throughout all operations
   */
  test('should preserve unshielded offers through multiple operations', () => {
    const intent = Intent.new(TTL);
    const offer = getNewUnshieldedOffer();

    intent.guaranteedUnshieldedOffer = offer;
    intent.fallibleUnshieldedOffer = offer;

    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    let updatedIntent = intent.addDeploy(contractDeploy);

    const commitmentRandomness = communicationCommitmentRandomness();
    const contractAddress = Random.contractAddress();
    const contractCallPrototype = new ContractCallPrototype(
      contractAddress,
      'entry',
      new ContractOperation(),
      undefined,
      undefined,
      [Static.alignedValue],
      Static.alignedValue,
      Static.alignedValue,
      commitmentRandomness,
      'key_location'
    );
    updatedIntent = updatedIntent.addCall(contractCallPrototype);

    expect(updatedIntent.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
    expect(updatedIntent.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());

    const serialized = updatedIntent.serialize();
    const deserialized = Intent.deserialize('signature', 'pre-proof', 'pre-binding', serialized);

    expect(deserialized.guaranteedUnshieldedOffer?.toString()).toEqual(offer.toString());
    expect(deserialized.fallibleUnshieldedOffer?.toString()).toEqual(offer.toString());
  });

  /**
   * Test setting and getting guaranteed unshielded offer with multiple inputs and outputs.
   *
   * @given Multiple UTXO spends with different values and output numbers
   * @and Multiple outputs with varied amounts and owner addresses
   * @when Setting guaranteedUnshieldedOffer with complex multi-input/output structure
   * @and Verifying input and output collections
   * @then Should properly store offer with correct input/output counts
   * @and Should maintain all UTXO spend and output details
   */
  test('should set and get guaranteedUnshieldedOffer with multiple inputs and outputs', () => {
    const intent = Intent.new(TTL);
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = Random.signatureVerifyingKeyNew();
    const signature1 = Random.signature();
    const signature2 = Random.signature();

    const utxoSpend1 = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const utxoSpend2 = {
      value: 50n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 1
    };

    const utxoOutput1 = {
      value: 75n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const utxoOutput2 = {
      value: 75n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [utxoSpend1, utxoSpend2],
      [utxoOutput1, utxoOutput2],
      [signature1, signature2]
    );

    expect(intent.guaranteedUnshieldedOffer.inputs.length).toEqual(2);
    expect(intent.guaranteedUnshieldedOffer.outputs.length).toEqual(2);
    expect(intent.guaranteedUnshieldedOffer.signatures.length).toEqual(2);

    expect(intent.guaranteedUnshieldedOffer.inputs).toContainEqual(utxoSpend1);
    expect(intent.guaranteedUnshieldedOffer.inputs).toContainEqual(utxoSpend2);
    expect(intent.guaranteedUnshieldedOffer.outputs).toContainEqual(utxoOutput1);
    expect(intent.guaranteedUnshieldedOffer.outputs).toContainEqual(utxoOutput2);
  });

  /**
   * Test setting and getting fallible unshielded offer with multiple inputs and outputs.
   *
   * @given Multiple UTXO spends with varying values and output numbers
   * @and Multiple outputs with different amounts and owner addresses
   * @when Setting fallibleUnshieldedOffer with complex input/output structure
   * @and Validating offer properties and collections
   * @then Should properly store offer with correct input/output counts
   * @and Should maintain all UTXO spend details across multiple inputs
   */
  test('should set and get fallibleUnshieldedOffer with multiple inputs and outputs', () => {
    const intent = Intent.new(TTL);
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = Random.signatureVerifyingKeyNew();
    const signature1 = Random.signature();
    const signature2 = Random.signature();

    const utxoSpend1 = {
      value: 200n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 2
    };

    const utxoSpend2 = {
      value: 300n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 3
    };

    const utxoSpend3 = {
      value: 300n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 4
    };

    const utxoOutput1 = {
      value: 250n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const utxoOutput2 = {
      value: 250n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    intent.fallibleUnshieldedOffer = UnshieldedOffer.new(
      [utxoSpend1, utxoSpend2, utxoSpend3],
      [utxoOutput1, utxoOutput2],
      [signature1, signature2]
    );

    expect(intent.fallibleUnshieldedOffer.inputs.length).toEqual(3);
    expect(intent.fallibleUnshieldedOffer.outputs.length).toEqual(2);
    expect(intent.fallibleUnshieldedOffer.signatures.length).toEqual(2);

    expect(intent.fallibleUnshieldedOffer.inputs).toContainEqual(utxoSpend1);
    expect(intent.fallibleUnshieldedOffer.inputs).toContainEqual(utxoSpend2);
    expect(intent.fallibleUnshieldedOffer.outputs).toContainEqual(utxoOutput1);
    expect(intent.fallibleUnshieldedOffer.outputs).toContainEqual(utxoOutput2);
  });

  /**
   * Test handling both guaranteed and fallible offers with multiple inputs and outputs.
   *
   * @given Multiple UTXO spends and outputs for both guaranteed and fallible offers
   * @and Different token types for each offer type
   * @when Setting both guaranteed and fallible offers with complex input/output structures
   * @and Performing serialization and deserialization operations
   * @then Should preserve both offers with correct input/output counts
   * @and Should maintain all UTXO spend and output content integrity
   */
  test('should handle both guaranteed and fallible offers with multiple inputs and outputs', () => {
    const intent = Intent.new(TTL);
    const intentHash = sampleIntentHash();
    const token1 = Random.unshieldedTokenType();
    const token2 = Random.unshieldedTokenType();
    const svk = Random.signatureVerifyingKeyNew();
    const signature1 = Random.signature();
    const signature2 = Random.signature();

    // Create guaranteed offer with multiple inputs/outputs
    const guaranteedSpend1 = {
      value: 100n,
      owner: svk,
      type: token1.raw,
      intentHash,
      outputNo: 0
    };

    const guaranteedSpend2 = {
      value: 200n,
      owner: svk,
      type: token1.raw,
      intentHash,
      outputNo: 1
    };

    const guaranteedOutput1 = {
      value: 150n,
      owner: sampleUserAddress(),
      type: token1.raw
    };

    const guaranteedOutput2 = {
      value: 150n,
      owner: sampleUserAddress(),
      type: token1.raw
    };

    const guaranteedOffer = UnshieldedOffer.new(
      [guaranteedSpend1, guaranteedSpend2],
      [guaranteedOutput1, guaranteedOutput2],
      [signature1, signature2]
    );

    // Create fallible offer with multiple inputs/outputs
    const fallibleSpend1 = {
      value: 300n,
      owner: svk,
      type: token2.raw,
      intentHash,
      outputNo: 2
    };

    const fallibleSpend2 = {
      value: 400n,
      owner: svk,
      type: token2.raw,
      intentHash,
      outputNo: 3
    };

    const fallibleOutput1 = {
      value: 350n,
      owner: sampleUserAddress(),
      type: token2.raw
    };

    const fallibleOutput2 = {
      value: 350n,
      owner: sampleUserAddress(),
      type: token2.raw
    };

    const fallibleOffer = UnshieldedOffer.new(
      [fallibleSpend1, fallibleSpend2],
      [fallibleOutput1, fallibleOutput2],
      [signature1, signature2]
    );

    // Set both offers on the intent
    intent.guaranteedUnshieldedOffer = guaranteedOffer;
    intent.fallibleUnshieldedOffer = fallibleOffer;

    // Verify guaranteed offer properties
    expect(intent.guaranteedUnshieldedOffer.inputs.length).toEqual(2);
    expect(intent.guaranteedUnshieldedOffer.outputs.length).toEqual(2);
    expect(intent.guaranteedUnshieldedOffer.signatures.length).toEqual(2);
    expect(intent.guaranteedUnshieldedOffer.inputs).toContainEqual(guaranteedSpend1);
    expect(intent.guaranteedUnshieldedOffer.inputs).toContainEqual(guaranteedSpend2);
    expect(intent.guaranteedUnshieldedOffer.outputs).toContainEqual(guaranteedOutput1);
    expect(intent.guaranteedUnshieldedOffer.outputs).toContainEqual(guaranteedOutput2);

    // Verify fallible offer properties
    expect(intent.fallibleUnshieldedOffer.inputs.length).toEqual(2);
    expect(intent.fallibleUnshieldedOffer.outputs.length).toEqual(2);
    expect(intent.fallibleUnshieldedOffer.signatures.length).toEqual(2);
    expect(intent.fallibleUnshieldedOffer.inputs).toContainEqual(fallibleSpend1);
    expect(intent.fallibleUnshieldedOffer.inputs).toContainEqual(fallibleSpend2);
    expect(intent.fallibleUnshieldedOffer.outputs).toContainEqual(fallibleOutput1);
    expect(intent.fallibleUnshieldedOffer.outputs).toContainEqual(fallibleOutput2);

    // Ensure offers are preserved after serialization/deserialization
    const serialized = intent.serialize();
    const deserialized = Intent.deserialize('signature', 'pre-proof', 'pre-binding', serialized);

    // Check that both offers are correctly preserved
    expect(deserialized.guaranteedUnshieldedOffer?.toString()).toEqual(guaranteedOffer.toString());
    expect(deserialized.fallibleUnshieldedOffer?.toString()).toEqual(fallibleOffer.toString());

    // Verify input and output counts are preserved
    expect(deserialized.guaranteedUnshieldedOffer?.inputs.length).toEqual(2);
    expect(deserialized.guaranteedUnshieldedOffer?.outputs.length).toEqual(2);
    expect(deserialized.fallibleUnshieldedOffer?.inputs.length).toEqual(2);
    expect(deserialized.fallibleUnshieldedOffer?.outputs.length).toEqual(2);
  });

  /**
   * Test preservation of multi-input/output offers when binding intent.
   *
   * @given An intent with multiple input/output guaranteed and fallible offers
   * @and Both offers contain multiple UTXO spends and outputs
   * @when Binding the intent with segment ID 1
   * @and Verifying bound intent properties
   * @then Should preserve all offer input/output counts after binding
   * @and Should maintain guaranteed and fallible offer content integrity
   */
  test('should preserve multi-input/output offers when binding intent', () => {
    const intent = Intent.new(TTL);
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = Random.signatureVerifyingKeyNew();
    const signature1 = Random.signature();
    const signature2 = Random.signature();

    // Create guaranteed offer with multiple inputs/outputs
    const guaranteedSpend1 = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const guaranteedSpend2 = {
      value: 200n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 1
    };

    const guaranteedOutput1 = {
      value: 150n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const guaranteedOutput2 = {
      value: 150n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const guaranteedOffer = UnshieldedOffer.new(
      [guaranteedSpend1, guaranteedSpend2],
      [guaranteedOutput1, guaranteedOutput2],
      [signature1, signature2]
    );

    // Create fallible offer with multiple inputs/outputs
    const fallibleSpend1 = {
      value: 300n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 2
    };

    const fallibleSpend2 = {
      value: 400n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 3
    };

    const fallibleOutput1 = {
      value: 350n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const fallibleOutput2 = {
      value: 350n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const fallibleOffer = UnshieldedOffer.new(
      [fallibleSpend1, fallibleSpend2],
      [fallibleOutput1, fallibleOutput2],
      [signature1, signature2]
    );

    // Set both offers on the intent
    intent.guaranteedUnshieldedOffer = guaranteedOffer;
    intent.fallibleUnshieldedOffer = fallibleOffer;

    // Bind the intent
    const boundIntent = intent.bind(1);

    // Verify the offers are preserved after binding
    expect(boundIntent.guaranteedUnshieldedOffer?.inputs.length).toEqual(2);
    expect(boundIntent.guaranteedUnshieldedOffer?.outputs.length).toEqual(2);
    expect(boundIntent.fallibleUnshieldedOffer?.inputs.length).toEqual(2);
    expect(boundIntent.fallibleUnshieldedOffer?.outputs.length).toEqual(2);

    // Verify the content of the offers is preserved
    expect(boundIntent.guaranteedUnshieldedOffer?.inputs).toContainEqual(guaranteedSpend1);
    expect(boundIntent.guaranteedUnshieldedOffer?.inputs).toContainEqual(guaranteedSpend2);
    expect(boundIntent.guaranteedUnshieldedOffer?.outputs).toContainEqual(guaranteedOutput1);
    expect(boundIntent.guaranteedUnshieldedOffer?.outputs).toContainEqual(guaranteedOutput2);

    expect(boundIntent.fallibleUnshieldedOffer?.inputs).toContainEqual(fallibleSpend1);
    expect(boundIntent.fallibleUnshieldedOffer?.inputs).toContainEqual(fallibleSpend2);
    expect(boundIntent.fallibleUnshieldedOffer?.outputs).toContainEqual(fallibleOutput1);
    expect(boundIntent.fallibleUnshieldedOffer?.outputs).toContainEqual(fallibleOutput2);
  });

  /**
   * Test serialization and deserialization of bound intent.
   *
   * @given A bound intent with signature data
   * @when Serializing and deserializing the bound intent
   * @then Should maintain identical string representation after deserialization
   */
  test('should correctly serialize and deserialize bound intent', () => {
    const intent = Intent.new(TTL);
    const boundIntent = intent.bind(1);
    const signatureData = boundIntent.signatureData(1);

    expect(signatureData.length).toBeGreaterThan(0);

    const serialized = boundIntent.serialize();
    const deserialized = Intent.deserialize('signature', 'pre-proof', 'binding', serialized);

    expect(deserialized.toString()).toEqual(boundIntent.toString());
  });

  /**
   * Test serialization and deserialization with various parameter combinations.
   *
   * @given Different combinations of signature, preProof, and binding parameters
   * @when Serializing and deserializing intent with each combination
   * @then Should either succeed with 'OK' result or throw expected error messages
   */
  test.each([
    ['signature', 'pre-proof', 'pre-binding', 'OK'],
    ['signature', 'pre-proof', 'binding', 'Unable to deserialize Intent'],
    ['signature', 'pre-proof', 'no-binding', 'Unsupported intent type provided.'],
    ['signature', 'proof', 'pre-binding', 'Unable to deserialize Intent'],
    ['signature', 'proof', 'binding', 'Unable to deserialize Intent'],
    ['signature', 'proof', 'no-binding', 'Unsupported intent type provided.'],
    ['signature', 'no-proof', 'pre-binding', 'Unsupported intent type provided.'],
    ['signature', 'no-proof', 'binding', 'Unsupported intent type provided.'],
    ['signature', 'no-proof', 'no-binding', 'Unable to deserialize Intent'],
    ['signature-erased', 'pre-proof', 'pre-binding', 'Unable to deserialize Intent'],
    ['signature-erased', 'pre-proof', 'binding', 'Unable to deserialize Intent'],
    ['signature-erased', 'pre-proof', 'no-binding', 'Unsupported intent type provided.'],
    ['signature-erased', 'proof', 'pre-binding', 'Unable to deserialize Intent'],
    ['signature-erased', 'proof', 'binding', 'Unable to deserialize Intent'],
    ['signature-erased', 'proof', 'no-binding', 'Unsupported intent type provided.'],
    ['signature-erased', 'no-proof', 'pre-binding', 'Unsupported intent type provided.'],
    ['signature-erased', 'no-proof', 'binding', 'Unsupported intent type provided.'],
    ['signature-erased', 'no-proof', 'no-binding', 'Unable to deserialize Intent']
  ])(
    'should handle serialization and deserialization with %s, %s, %s expecting %s',
    (signature, preProof, binding, expectedResult) => {
      const intent = Intent.new(new Date(TTL));

      const serialized = intent.serialize();

      if (expectedResult === 'OK') {
        const deserialized = Intent.deserialize(
          signature as 'signature' | 'signature-erased',
          preProof as 'pre-proof' | 'proof' | 'no-proof',
          binding as 'pre-binding' | 'binding' | 'no-binding',
          serialized
        );
        expect(deserialized.toString()).toEqual(intent.toString());
      } else {
        expect(() => {
          Intent.deserialize(
            signature as 'signature' | 'signature-erased',
            preProof as 'pre-proof' | 'proof' | 'no-proof',
            binding as 'pre-binding' | 'binding' | 'no-binding',
            serialized
          );
        }).toThrow(expectedResult);
      }
    }
  );
});
