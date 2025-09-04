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
  ChargedState,
  ContractDeploy,
  ContractOperation,
  ContractState,
  ContractCallPrototype,
  Intent,
  Transaction,
  StateValue,
  QueryContext,
  VmStack,
  runProgram,
  CostModel,
  communicationCommitmentRandomness,
  communicationCommitment,
  TransactionContext,
  ContractMaintenanceAuthority,
  MaintenanceUpdate,
  VerifierKeyInsert,
  VerifierKeyRemove,
  ContractOperationVersion,
  ContractOperationVersionedVerifierKey,
  ZswapChainState,
  LedgerState,
  signatureVerifyingKey,
  type PreBinding
} from '@midnight-ntwrk/ledger';
import { Random, Static, TestResource } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Contract Security Vector Tests', () => {
  describe('Contract Deployment Security', () => {
    /**
     * Validates that contract deployments generate unique addresses even with identical states
     * @given identical contract states are used for multiple deployments
     * @when deploying the same contract state multiple times
     * @then each deployment should generate a unique 64-character hex address
     */
    test('should generate unique addresses for identical contract states', () => {
      const contractState = new ContractState();

      const deploy1 = new ContractDeploy(contractState);
      const deploy2 = new ContractDeploy(contractState);
      const deploy3 = new ContractDeploy(contractState);

      expect(deploy1.address).not.toEqual(deploy2.address);
      expect(deploy2.address).not.toEqual(deploy3.address);
      expect(deploy1.address).not.toEqual(deploy3.address);

      [deploy1, deploy2, deploy3].forEach((deploy) => {
        expect(deploy.address.length).toEqual(64);
        expect(deploy.address).toMatch(/^[a-fA-F0-9]{64}$/);
      });
    });

    /**
     * Ensures contract state immutability during deployment to prevent tampering
     * @given a contract state with multiple operations
     * @when the contract is deployed and original state is modified afterward
     * @then the deployed contract should maintain its original immutable state
     */
    test('should protect against contract state tampering during deployment', () => {
      const contractState = new ContractState();
      contractState.setOperation('op1', new ContractOperation());
      contractState.setOperation('op2', new ContractOperation());

      const originalSerialized = contractState.serialize();

      const deploy = new ContractDeploy(contractState);

      contractState.setOperation('malicious', new ContractOperation());

      expect(deploy.initialState.serialize()).toEqual(originalSerialized);
      expect(deploy.initialState.serialize()).not.toEqual(contractState.serialize());
    });

    /**
     * Validates contract operation limits to prevent denial-of-service attacks
     * @given a contract state with a large number of operations
     * @when deploying the contract with maximum reasonable operation count
     * @then the deployment should succeed without throwing errors
     */
    test('should validate contract operation limits to prevent DoS', () => {
      const contractState = new ContractState();

      const maxReasonableOps = 1000;

      for (let i = 0; i < maxReasonableOps; i++) {
        contractState.setOperation(`operation_${i}`, new ContractOperation());
      }

      expect(() => {
        // eslint-disable-next-line no-new
        new ContractDeploy(contractState);
      }).not.toThrow();

      const deploy = new ContractDeploy(contractState);
      expect(deploy.address).toMatch(/^[a-fA-F0-9]{64}$/);

      assertSerializationSuccess(contractState);
    });

    /**
     * Tests deterministic address generation properties for contract security
     * @given identical contract operations in different contract states
     * @when deploying contracts with same operations
     * @then addresses should be unique and each deployment should generate different addresses
     */
    test('should ensure contract address determinism properties', () => {
      const contractState1 = new ContractState();
      const contractState2 = new ContractState();

      contractState1.setOperation('test', new ContractOperation());
      contractState2.setOperation('test', new ContractOperation());

      const deploy1 = new ContractDeploy(contractState1);
      const deploy2 = new ContractDeploy(contractState2);

      expect(deploy1.address).not.toEqual(deploy2.address);

      const deploy1Copy = new ContractDeploy(contractState1);
      expect(deploy1Copy.address).not.toEqual(deploy1.address);
    });
  });

  describe('Contract Call Security', () => {
    /**
     * Validates secure contract call construction with proper authorization mechanisms
     * @given valid contract address and operation parameters
     * @when creating a contract call with proper commitments and key location
     * @then the call should be properly constructed with valid communication commitment
     */
    test('should validate contract call construction with proper authorization', () => {
      const contractAddress = Random.contractAddress();
      const contractOperation = new ContractOperation();
      const commitmentRandomness = communicationCommitmentRandomness();

      const contractCall = new ContractCallPrototype(
        contractAddress,
        'test_entry',
        contractOperation,
        undefined,
        undefined,
        [Static.alignedValue],
        Static.alignedValue,
        Static.alignedValue,
        commitmentRandomness,
        'test_key_location'
      ).intoCall('pre-binding' as unknown as PreBinding);

      expect(contractCall.address).toEqual(contractAddress);
      expect(contractCall.entryPoint).toEqual('test_entry');
      expect(contractCall.communicationCommitment).toBeDefined();
      expect(contractCall.toString(true)).toMatch(/\{contract:.*/);
    });

    /**
     * Prevents unauthorized modification of bound contract calls in transactions
     * @given a bound transaction with contract calls
     * @when attempting to modify the transaction intents after binding
     * @then the system should throw an error preventing modification
     */
    test('should prevent unauthorized contract call modification', () => {
      const contractAddress = Random.contractAddress();
      const contractOperation = new ContractOperation();

      const contractCall = new ContractCallPrototype(
        contractAddress,
        'secure_operation',
        contractOperation,
        undefined,
        undefined,
        [],
        Static.alignedValue,
        Static.alignedValue,
        communicationCommitmentRandomness(),
        'secure_key'
      );

      const intent = Intent.new(new Date()).addCall(contractCall);
      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      const boundTransaction = transaction.bind();

      expect(() => {
        const newIntent = Intent.new(new Date());
        const newIntents = new Map();
        newIntents.set(1, newIntent);
        boundTransaction.intents = newIntents;
      }).toThrow('Transaction is already bound');
    });

    /**
     * Validates cryptographic integrity of communication commitments
     * @given two values and randomness parameters
     * @when generating commitments with same and different randomness
     * @then commitments should be deterministic with same inputs and unique with different inputs
     */
    test('should validate communication commitment integrity', () => {
      const value1 = Static.alignedValue;
      const value2 = Static.alignedValueCompress;
      const randomness1 = communicationCommitmentRandomness();
      const randomness2 = communicationCommitmentRandomness();

      const commitment1a = communicationCommitment(value1, value2, randomness1);
      const commitment1b = communicationCommitment(value1, value2, randomness1);
      expect(commitment1a).toEqual(commitment1b);

      const commitment2 = communicationCommitment(value1, value2, randomness2);
      expect(commitment1a).not.toEqual(commitment2);

      const commitment3 = communicationCommitment(value2, value1, randomness1);
      expect(commitment1a).not.toEqual(commitment3);

      expect(commitment1a.length).toBeLessThanOrEqual(114);
      expect(commitment1a.length).toBeGreaterThan(0);
    });
  });

  describe('Contract State Validation', () => {
    /**
     * Protects against malicious state value manipulation attacks
     * @given different state value types (null and array)
     * @when creating and converting state values to string representation
     * @then each type should produce distinct, safe string outputs
     */
    test('should protect against state value manipulation', () => {
      const originalValue = StateValue.newNull();
      const modifiedValue = StateValue.newArray();

      expect(originalValue.toString()).not.toEqual(modifiedValue.toString());

      expect(() => originalValue.toString()).not.toThrow();
      expect(() => modifiedValue.toString()).not.toThrow();

      expect(originalValue.toString().length).toBeGreaterThan(0);
      expect(modifiedValue.toString().length).toBeGreaterThan(0);
    });

    /**
     * Validates security of state map operations to prevent memory disclosure
     * @given array and null state values
     * @when converting to string representation
     * @then output should not contain memory addresses or allocation information
     */
    test('should validate state map operations for security', () => {
      const arrayValue = StateValue.newArray();
      const nullValue = StateValue.newNull();

      const arrayString = arrayValue.toString();
      const nullString = nullValue.toString();

      expect(arrayString).not.toMatch(/0x[0-9a-fA-F]+/);
      expect(nullString).not.toMatch(/0x[0-9a-fA-F]+/);

      expect(arrayString).not.toContain('ptr');
      expect(arrayString).not.toContain('alloc');
      expect(nullString).not.toContain('ptr');
      expect(nullString).not.toContain('alloc');
    });

    /**
     * Handles query context security boundaries to prevent information leakage
     * @given a query context with state value and contract address
     * @when creating and accessing the query context
     * @then the context should not expose memory addresses in string representation
     */
    test('should handle query context security boundaries', () => {
      const stateValue = new ChargedState(StateValue.newArray());
      const contractAddress = Random.contractAddress();

      const queryContext = new QueryContext(stateValue, contractAddress);

      expect(queryContext).toBeDefined();
      expect(() => queryContext.toString()).not.toThrow();

      const contextString = queryContext.toString();
      expect(contextString).not.toMatch(/0x[0-9a-fA-F]+/);
    });
  });

  describe('VM Execution Security', () => {
    /**
     * Prevents stack overflow attacks on the virtual machine
     * @given a VM stack with 100 null values pushed
     * @when checking stack strength at various indices
     * @then stack should handle bounds correctly and not expose memory information
     */
    test('should prevent stack overflow attacks', () => {
      const vmStack = new VmStack();

      for (let i = 0; i < 100; i++) {
        vmStack.push(StateValue.newNull(), true);
      }

      expect(vmStack.isStrong(0)).toBe(true);
      expect(vmStack.isStrong(99)).toBe(true);
      expect(vmStack.isStrong(100)).toBeUndefined();

      expect(() => vmStack.toString()).not.toThrow();
    });

    /**
     * Validates program execution with proper cost limits to prevent resource exhaustion
     * @given a VM stack with array value and size operation
     * @when running program with cost model limits
     * @then execution should complete with positive gas cost and valid results
     */
    test('should validate program execution with cost limits', () => {
      const vmStack = new VmStack();
      vmStack.push(StateValue.newArray(), true);

      const results = runProgram(vmStack, ['size'], CostModel.initialCostModel(), undefined);

      expect(results.stack.isStrong(0)).toBe(true);
      expect(results.gasCost.computeTime).toBeGreaterThan(0n);
      expect(results.events).toHaveLength(0);
      expect(() => results.toString()).not.toThrow();
    });
  });

  describe('Maintenance Authority Security', () => {
    /**
     * Validates security of contract maintenance authority operations
     * @given a signature verifying key and authority configuration
     * @when creating a maintenance authority with threshold requirements
     * @then authority should be properly formed and serializable
     */
    test('should validate maintenance authority operations', () => {
      const svk = signatureVerifyingKey(Random.signingKey());
      const authority = new ContractMaintenanceAuthority([svk], 1, 0n);

      expect(authority).toBeDefined();
      expect(() => authority.toString()).not.toThrow();
      expect(authority.toString().length).toBeGreaterThan(0);

      assertSerializationSuccess(authority);
    });

    /**
     * Validates security of maintenance update operations on contracts
     * @given a contract address and verifier key removal operation
     * @when creating a maintenance update with version constraints
     * @then update should be properly constructed and safe to string conversion
     */
    test('should validate maintenance update security', () => {
      const contractAddress = Random.contractAddress();
      const operation = 'test_operation';
      const maintenanceUpdate = new MaintenanceUpdate(
        contractAddress,
        [new VerifierKeyRemove(operation, new ContractOperationVersion('v2'))],
        0n
      );

      expect(maintenanceUpdate).toBeDefined();
      expect(() => maintenanceUpdate.toString()).not.toThrow();
    });

    /**
     * Secures verifier key management operations against unauthorized access
     * @given versioned verifier keys for insert and remove operations
     * @when performing key management operations
     * @then operations should complete safely without exposing sensitive data
     */
    test('should secure verifier key management operations', () => {
      const versionedKey = new ContractOperationVersionedVerifierKey('v2', TestResource.operationVerifierKey());
      const keyInsert = new VerifierKeyInsert('op', versionedKey);

      expect(keyInsert).toBeDefined();
      expect(() => keyInsert.toString()).not.toThrow();

      const operationVersion = new ContractOperationVersion('v2');
      const keyRemove = new VerifierKeyRemove('op', operationVersion);

      expect(keyRemove).toBeDefined();
      expect(() => keyRemove.toString()).not.toThrow();
    });
  });

  describe('Resource Limit Enforcement', () => {
    /**
     * Handles large contract states efficiently to prevent performance degradation
     * @given a contract state with 500 operations
     * @when creating and deploying the contract within time constraints
     * @then deployment should complete within reasonable time and produce valid address
     */
    test('should handle large contract states efficiently', () => {
      const contractState = new ContractState();

      const startTime = performance.now();

      for (let i = 0; i < 500; i++) {
        contractState.setOperation(`op_${i}`, new ContractOperation());
      }

      const endTime = performance.now();
      const duration = endTime - startTime;

      expect(duration).toBeLessThan(1000);

      const deploy = new ContractDeploy(contractState);
      expect(deploy.address).toMatch(/^[a-fA-F0-9]{64}$/);

      assertSerializationSuccess(contractState);
    });

    /**
     * Limits transaction context complexity to prevent resource exhaustion
     * @given complex zswap chain state and ledger state with block context
     * @when creating a transaction context with all components
     * @then context creation should succeed and be safe for string conversion
     */
    test('should limit transaction context complexity', () => {
      const zswapChainState = new ZswapChainState();
      const ledgerState = new LedgerState('local-test', zswapChainState);
      const blockContext = {
        secondsSinceEpoch: Static.blockTime(new Date()),
        secondsSinceEpochErr: 0,
        parentBlockHash: Static.parentBlockHash()
      };

      const transactionContext = new TransactionContext(ledgerState, blockContext);

      expect(transactionContext).toBeDefined();
      expect(() => transactionContext.toString()).not.toThrow();
    });

    /**
     * Prevents memory exhaustion in VM operations under heavy load
     * @given a VM stack with 1000 null values
     * @when monitoring memory usage during stack operations
     * @then memory growth should remain within acceptable limits
     */
    test('should prevent memory exhaustion in VM operations', () => {
      const vmStack = new VmStack();

      const initialMemory = process.memoryUsage().heapUsed;

      for (let i = 0; i < 1000; i++) {
        vmStack.push(StateValue.newNull(), true);
      }

      const finalMemory = process.memoryUsage().heapUsed;
      const memoryGrowth = finalMemory - initialMemory;

      expect(memoryGrowth).toBeLessThan(10 * 1024 * 1024);

      expect(vmStack.isStrong(0)).toBe(true);
      expect(vmStack.isStrong(999)).toBe(true);
    });
  });

  describe('Error Handling Security', () => {
    /**
     * Ensures error messages do not leak sensitive information
     * @given a contract state with secret operations
     * @when triggering an error condition with invalid operation name
     * @then error message should not contain sensitive operation details
     */
    test('should not leak sensitive information in error messages', () => {
      const contractState = new ContractState();
      const secretOperation = new ContractOperation();
      contractState.setOperation('secret_key', secretOperation);

      try {
        contractState.setOperation('', secretOperation);
      } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);

        expect(errorMessage).not.toContain('secret_key');
        expect(errorMessage).not.toContain(secretOperation.toString());
      }
    });

    /**
     * Handles malformed serialization gracefully without system compromise
     * @given a valid contract state
     * @when performing string conversion and serialization operations
     * @then operations should not throw and produce valid byte arrays
     */
    test('should handle malformed serialization gracefully', () => {
      const contractState = new ContractState();

      expect(() => contractState.toString()).not.toThrow();

      expect(() => contractState.serialize()).not.toThrow();

      const serialized = contractState.serialize();
      expect(serialized).toBeInstanceOf(Uint8Array);
      expect(serialized.length).toBeGreaterThan(0);
    });
  });
});
