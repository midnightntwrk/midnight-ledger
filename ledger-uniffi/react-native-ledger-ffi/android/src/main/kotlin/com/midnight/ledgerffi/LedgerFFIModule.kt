package com.midnight.ledgerffi

import uniffi.ledger_uniffi.*
import com.facebook.react.bridge.*
import com.facebook.react.modules.core.DeviceEventManagerModule

class LedgerFFIModule(reactContext: ReactApplicationContext) : ReactContextBaseJavaModule(reactContext) {
    
    override fun getName(): String = "LedgerFFI"

    @ReactMethod
    fun hello(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.hello()
            promise.resolve(result)
        } catch (e: Exception) {
            promise.reject("HELLO_ERROR", "Failed to call hello: ${e.message}", e)
        }
    }

    @ReactMethod
    fun nativeToken(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.nativeToken()
            promise.resolve(result.name)
        } catch (e: Exception) {
            promise.reject("NATIVE_TOKEN_ERROR", "Failed to call nativeToken: ${e.message}", e)
        }
    }

    @ReactMethod
    fun feeToken(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.feeToken()
            promise.resolve(result.name)
        } catch (e: Exception) {
            promise.reject("FEE_TOKEN_ERROR", "Failed to call feeToken: ${e.message}", e)
        }
    }

    @ReactMethod
    fun shieldedToken(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.shieldedToken()
            promise.resolve(result.name)
        } catch (e: Exception) {
            promise.reject("SHIELDED_TOKEN_ERROR", "Failed to call shieldedToken: ${e.message}", e)
        }
    }

    @ReactMethod
    fun unshieldedToken(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.unshieldedToken()
            promise.resolve(result.name)
        } catch (e: Exception) {
            promise.reject("UNSHIELDED_TOKEN_ERROR", "Failed to call unshieldedToken: ${e.message}", e)
        }
    }

    @ReactMethod
    fun sampleCoinPublicKey(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.sampleCoinPublicKey()
            val hashOutput = WritableNativeMap()
            hashOutput.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
            val publicKey = WritableNativeMap()
            publicKey.putMap("hash", hashOutput)
            promise.resolve(publicKey)
        } catch (e: Exception) {
            promise.reject("SAMPLE_COIN_PUBLIC_KEY_ERROR", "Failed to call sampleCoinPublicKey: ${e.message}", e)
        }
    }

    @ReactMethod
    fun sampleEncryptionPublicKey(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.sampleEncryptionPublicKey()
            promise.resolve(result)
        } catch (e: Exception) {
            promise.reject("SAMPLE_ENCRYPTION_PUBLIC_KEY_ERROR", "Failed to call sampleEncryptionPublicKey: ${e.message}", e)
        }
    }

    @ReactMethod
    fun sampleIntentHash(promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.sampleIntentHash()
            val intentHash = WritableNativeMap()
            intentHash.putArray("hash", Arguments.fromArray(result.`hash`.toTypedArray()))
            promise.resolve(intentHash)
        } catch (e: Exception) {
            promise.reject("SAMPLE_INTENT_HASH_ERROR", "Failed to call sampleIntentHash: ${e.message}", e)
        }
    }

    // Type creation functions
    @ReactMethod
    fun createShieldedCoinInfo(tokenTypeMap: ReadableMap, value: Int, promise: Promise) {
        try {
            val tokenTypeHash = tokenTypeMap.getMap("hash")!!
            val tokenTypeBytes = tokenTypeHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val tokenType = ShieldedTokenType(HashOutputWrapper(tokenTypeBytes))
            val result = uniffi.ledger_uniffi.createShieldedCoinInfo(tokenType, value.toLong())
            
            val nonceMap = WritableNativeMap()
            val nonceHash = WritableNativeMap()
            nonceHash.putArray("bytes", Arguments.fromArray(result.nonce.`hash`.bytes.toTypedArray()))
            nonceMap.putMap("hash", nonceHash)
            
            val tokenTypeResultMap = WritableNativeMap()
            val tokenTypeResultHash = WritableNativeMap()
            tokenTypeResultHash.putArray("bytes", Arguments.fromArray(result.tokenType.`hash`.bytes.toTypedArray()))
            tokenTypeResultMap.putMap("hash", tokenTypeResultHash)
            
            val coinInfo = WritableNativeMap()
            coinInfo.putMap("nonce", nonceMap)
            coinInfo.putMap("token_type", tokenTypeResultMap)
            coinInfo.putInt("value", result.value.toInt())
            
            promise.resolve(coinInfo)
        } catch (e: Exception) {
            promise.reject("CREATE_SHIELDED_COIN_INFO_ERROR", "Failed to call createShieldedCoinInfo: ${e.message}", e)
        }
    }

    // Cryptographic functions
    @ReactMethod
    fun coinNullifier(coinInfoMap: ReadableMap, coinSecretKey: String, promise: Promise) {
        try {
            // Convert coinInfoMap to ShieldedCoinInfo
            val nonceMap = coinInfoMap.getMap("nonce")!!
            val nonceHash = nonceMap.getMap("hash")!!
            val nonceBytes = nonceHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val nonce = uniffi.ledger_uniffi.Nonce(uniffi.ledger_uniffi.HashOutputWrapper(nonceBytes))
            
            val tokenTypeMap = coinInfoMap.getMap("token_type")!!
            val tokenTypeHash = tokenTypeMap.getMap("hash")!!
            val tokenTypeBytes = tokenTypeHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val tokenType = uniffi.ledger_uniffi.ShieldedTokenType(uniffi.ledger_uniffi.HashOutputWrapper(tokenTypeBytes))
            
            val value = coinInfoMap.getInt("value")
            val coinInfo = uniffi.ledger_uniffi.ShieldedCoinInfo(nonce, tokenType, value.toLong())
            
            val result = uniffi.ledger_uniffi.coinNullifier(coinInfo, coinSecretKey)
            promise.resolve(result)
        } catch (e: Exception) {
            promise.reject("COIN_NULLIFIER_ERROR", "Failed to call coinNullifier: ${e.message}", e)
        }
    }

    @ReactMethod
    fun coinCommitment(coinInfoMap: ReadableMap, coinPublicKeyMap: ReadableMap, promise: Promise) {
        try {
            // Convert coinInfoMap to ShieldedCoinInfo
            val nonceMap = coinInfoMap.getMap("nonce")!!
            val nonceHash = nonceMap.getMap("hash")!!
            val nonceBytes = nonceHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val nonce = uniffi.ledger_uniffi.Nonce(uniffi.ledger_uniffi.HashOutputWrapper(nonceBytes))
            
            val tokenTypeMap = coinInfoMap.getMap("token_type")!!
            val tokenTypeHash = tokenTypeMap.getMap("hash")!!
            val tokenTypeBytes = tokenTypeHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val tokenType = uniffi.ledger_uniffi.ShieldedTokenType(uniffi.ledger_uniffi.HashOutputWrapper(tokenTypeBytes))
            
            val value = coinInfoMap.getInt("value")
            val coinInfo = uniffi.ledger_uniffi.ShieldedCoinInfo(nonce, tokenType, value.toLong())
            
            // Convert coinPublicKeyMap to PublicKey
            val publicKeyHash = coinPublicKeyMap.getMap("hash")!!
            val publicKeyBytes = publicKeyHash.getArray("bytes")!!.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val coinPublicKey = uniffi.ledger_uniffi.PublicKey(uniffi.ledger_uniffi.HashOutputWrapper(publicKeyBytes))
            
            val result = uniffi.ledger_uniffi.coinCommitment(coinInfo, coinPublicKey)
            val commitmentMap = WritableNativeMap()
            val commitmentHash = WritableNativeMap()
            commitmentHash.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
            commitmentMap.putMap("hash", commitmentHash)
            promise.resolve(commitmentMap)
        } catch (e: Exception) {
            promise.reject("COIN_COMMITMENT_ERROR", "Failed to call coinCommitment: ${e.message}", e)
        }
    }

    @ReactMethod
    fun addressFromKey(key: String, promise: Promise) {
        try {
            val result = uniffi.ledger_uniffi.addressFromKey(key)
            val addressMap = WritableNativeMap()
            val addressHash = WritableNativeMap()
            addressHash.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
            addressMap.putMap("hash", addressHash)
            promise.resolve(addressMap)
        } catch (e: Exception) {
            promise.reject("ADDRESS_FROM_KEY_ERROR", "Failed to call addressFromKey: ${e.message}", e)
        }
    }

    // Type conversion functions
    @ReactMethod
    fun shieldedTokenTypeFromBytes(bytes: ReadableArray, promise: Promise) {
        try {
            val byteList = bytes.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val result = uniffi.ledger_uniffi.shieldedTokenTypeFromBytes(byteList)
            val hashOutput = WritableNativeMap()
            hashOutput.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
            val tokenType = WritableNativeMap()
            tokenType.putMap("hash", hashOutput)
            promise.resolve(tokenType)
        } catch (e: Exception) {
            promise.reject("SHIELDED_TOKEN_TYPE_FROM_BYTES_ERROR", "Failed to call shieldedTokenTypeFromBytes: ${e.message}", e)
        }
    }

    @ReactMethod
    fun publicKeyFromBytes(bytes: ReadableArray, promise: Promise) {
        try {
            val byteList = bytes.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val result = uniffi.ledger_uniffi.publicKeyFromBytes(byteList)
            val hashOutput = WritableNativeMap()
            hashOutput.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
            val publicKey = WritableNativeMap()
            publicKey.putMap("hash", hashOutput)
            promise.resolve(publicKey)
        } catch (e: Exception) {
            promise.reject("PUBLIC_KEY_FROM_BYTES_ERROR", "Failed to call publicKeyFromBytes: ${e.message}", e)
        }
    }

    // @ReactMethod
    // fun userAddressFromBytes(bytes: ReadableArray, promise: Promise) {
    //     try {
    //         val byteList = bytes.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
    //         val result = uniffi.ledger_uniffi.userAddressFromBytes(byteList)
    //         val hashOutput = WritableNativeMap()
    //         hashOutput.putArray("bytes", Arguments.fromArray(result.`hash`.bytes.toTypedArray()))
    //         val userAddress = WritableNativeMap()
    //         userAddress.putMap("hash", hashOutput)
    //         promise.resolve(userAddress)
    //     } catch (e: Exception) {
    //         promise.reject("USER_ADDRESS_FROM_BYTES_ERROR", "Failed to call userAddressFromBytes: ${e.message}", e)
    //     }
    // }

    // @ReactMethod
    // fun transactionHashFromBytes(bytes: ReadableArray, promise: Promise) {
    //     try {
    //         val byteList = bytes.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
    //         val result = uniffi.ledger_uniffi.transactionHashFromBytes(byteList)
    //         val transactionHash = WritableNativeMap()
    //         transactionHash.putArray("hash", Arguments.fromArray(result.`hash`.toTypedArray()))
    //         promise.resolve(transactionHash)
    //     } catch (e: Exception) {
    //         promise.reject("TRANSACTION_HASH_FROM_BYTES_ERROR", "Failed to call transactionHashFromBytes: ${e.message}", e)
    //     }
    // }

    @ReactMethod
    fun intentHashFromBytes(bytes: ReadableArray, promise: Promise) {
        try {
            val byteList = bytes.toArrayList().map { it as Int }.map { it.toByte() }.toByteArray()
            val result = uniffi.ledger_uniffi.intentHashFromBytes(byteList)
            val intentHash = WritableNativeMap()
            intentHash.putArray("hash", Arguments.fromArray(result.`hash`.toTypedArray()))
            promise.resolve(intentHash)
        } catch (e: Exception) {
            promise.reject("INTENT_HASH_FROM_BYTES_ERROR", "Failed to call intentHashFromBytes: ${e.message}", e)
        }
    }
}
