import Foundation
import React

// Create a simple wrapper to call UniFFI functions
// The functions are available globally in the same module
@objc(LedgerFFI)
class LedgerFFI: NSObject {
  
  @objc
  static func requiresMainQueueSetup() -> Bool {
    return false
  }
  
  @objc
  func constantsToExport() -> [AnyHashable: Any]! {
    return [:]
  }
  
  // Basic functions
  @objc
  func hello(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // Call the global hello function with module prefix to disambiguate
      let result = try react_native_ledger_ffi.hello()
      resolve(result)
    } catch {
      reject("HELLO_ERROR", "Failed to call hello: \(error.localizedDescription)", error)
    }
  }
  
  // Token type functions
  @objc
  func nativeToken(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.nativeToken()
      resolve(result)
    } catch {
      reject("NATIVE_TOKEN_ERROR", "Failed to call nativeToken: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func feeToken(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.feeToken()
      resolve(result)
    } catch {
      reject("FEE_TOKEN_ERROR", "Failed to call feeToken: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func shieldedToken(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.shieldedToken()
      resolve(result)
    } catch {
      reject("SHIELDED_TOKEN_ERROR", "Failed to call shieldedToken: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func unshieldedToken(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.unshieldedToken()
      resolve(result)
    } catch {
      reject("UNSHIELDED_TOKEN_ERROR", "Failed to call unshieldedToken: \(error.localizedDescription)", error)
    }
  }
  
  // Sample data functions
  @objc
  func sampleCoinPublicKey(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.sampleCoinPublicKey()
      resolve(result)
    } catch {
      reject("SAMPLE_COIN_PUBLIC_KEY_ERROR", "Failed to call sampleCoinPublicKey: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func sampleEncryptionPublicKey(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.sampleEncryptionPublicKey()
      resolve(result)
    } catch {
      reject("SAMPLE_ENCRYPTION_PUBLIC_KEY_ERROR", "Failed to call sampleEncryptionPublicKey: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func sampleIntentHash(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.sampleIntentHash()
      resolve(result)
    } catch {
      reject("SAMPLE_INTENT_HASH_ERROR", "Failed to call sampleIntentHash: \(error.localizedDescription)", error)
    }
  }
  
  // Type creation functions
  @objc
  func createShieldedCoinInfo(_ tokenType: String, value: NSInteger, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // For now, return a placeholder since this requires proper type conversion
      reject("UNSUPPORTED_VARIANT", "createShieldedCoinInfo requires proper type conversion", nil)
    } catch {
      reject("CREATE_SHIELDED_COIN_INFO_ERROR", "Failed to call createShieldedCoinInfo: \(error.localizedDescription)", error)
    }
  }
  
  // Type conversion functions
  @objc
  func shieldedTokenTypeFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.shieldedTokenTypeFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("SHIELDED_TOKEN_TYPE_FROM_BYTES_ERROR", "Failed to call shieldedTokenTypeFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func publicKeyFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.publicKeyFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("PUBLIC_KEY_FROM_BYTES_ERROR", "Failed to call publicKeyFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func userAddressFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.userAddressFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("USER_ADDRESS_FROM_BYTES_ERROR", "Failed to call userAddressFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func transactionHashFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.transactionHashFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("TRANSACTION_HASH_FROM_BYTES_ERROR", "Failed to call transactionHashFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func intentHashFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.intentHashFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("INTENT_HASH_FROM_BYTES_ERROR", "Failed to call intentHashFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  // Cryptographic functions
  @objc
  func coinNullifier(_ coinInfo: String, coinSecretKey: String, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // For now, return a placeholder since this requires proper type conversion
      reject("UNSUPPORTED_VARIANT", "coinNullifier requires proper type conversion", nil)
    } catch {
      reject("COIN_NULLIFIER_ERROR", "Failed to call coinNullifier: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func coinCommitment(_ coinInfo: String, coinPublicKey: String, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // For now, return a placeholder since this requires proper type conversion
      reject("UNSUPPORTED_VARIANT", "coinCommitment requires proper type conversion", nil)
    } catch {
      reject("COIN_COMMITMENT_ERROR", "Failed to call coinCommitment: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func addressFromKey(_ key: String, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let result = try react_native_ledger_ffi.addressFromKey(key: key)
      resolve(result)
    } catch {
      reject("ADDRESS_FROM_KEY_ERROR", "Failed to call addressFromKey: \(error.localizedDescription)", error)
    }
  }
  
  // Additional type conversion functions
  @objc
  func unshieldedTokenTypeFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.unshieldedTokenTypeFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("UNSHIELDED_TOKEN_TYPE_FROM_BYTES_ERROR", "Failed to call unshieldedTokenTypeFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func commitmentFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.commitmentFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("COMMITMENT_FROM_BYTES_ERROR", "Failed to call commitmentFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func nullifierFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.nullifierFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("NULLIFIER_FROM_BYTES_ERROR", "Failed to call nullifierFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func nonceFromBytes(_ bytes: NSArray, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      let byteList = bytes.map { ($0 as! NSNumber).uint8Value }
      let data = Data(byteList)
      let result = try react_native_ledger_ffi.nonceFromBytes(bytes: data)
      resolve(result)
    } catch {
      reject("NONCE_FROM_BYTES_ERROR", "Failed to call nonceFromBytes: \(error.localizedDescription)", error)
    }
  }
  
  // Transaction and proving functions
  @objc
  func createProvingTransactionPayload(_ transactionMap: NSDictionary, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // This is a complex function that would need proper transaction parsing
      // For now, return a placeholder error as in the Rust implementation
      reject("UNSUPPORTED_VARIANT", "createProvingTransactionPayload not yet implemented", nil)
    } catch {
      reject("CREATE_PROVING_TRANSACTION_PAYLOAD_ERROR", "Failed to call createProvingTransactionPayload: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func createProvingPayload(_ provingKeyMaterialMap: NSDictionary, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // This would need proper proving key material parsing
      reject("UNSUPPORTED_VARIANT", "createProvingPayload not yet implemented", nil)
    } catch {
      reject("CREATE_PROVING_PAYLOAD_ERROR", "Failed to call createProvingPayload: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func createCheckPayload(_ wrappedIrMap: NSDictionary, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // This would need proper wrapped IR parsing
      reject("UNSUPPORTED_VARIANT", "createCheckPayload not yet implemented", nil)
    } catch {
      reject("CREATE_CHECK_PAYLOAD_ERROR", "Failed to call createCheckPayload: \(error.localizedDescription)", error)
    }
  }
  
  @objc
  func parseCheckResult(_ resultMap: NSDictionary, resolver resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
      // This would need proper result parsing
      reject("UNSUPPORTED_VARIANT", "parseCheckResult not yet implemented", nil)
    } catch {
      reject("PARSE_CHECK_RESULT_ERROR", "Failed to call parseCheckResult: \(error.localizedDescription)", error)
    }
  }
}