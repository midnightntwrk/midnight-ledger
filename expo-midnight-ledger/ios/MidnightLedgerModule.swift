// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import ExpoModulesCore

/// Expo Module for Midnight Ledger functionality.
/// Provides access to shielded transactions, key management, and ledger operations.
public class MidnightLedgerModule: Module {
    public func definition() -> ModuleDefinition {
        Name("MidnightLedger")

        // MARK: - Constants (Token Types)

        Constants([
            "nativeToken": nativeToken(),
            "feeToken": feeToken(),
            "shieldedToken": shieldedToken(),
            "unshieldedToken": unshieldedToken()
        ])

        // MARK: - Utility Functions

        Function("sampleCoinPublicKey") { () -> String in
            return sampleCoinPublicKey()
        }

        Function("sampleEncryptionPublicKey") { () -> String in
            return sampleEncryptionPublicKey()
        }

        // MARK: - Key Derivation

        Function("signatureVerifyingKey") { (signingKey: String) throws -> String in
            do {
                let key = try signatureVerifyingKey(signingKey: signingKey)
                let keyId = UUID().uuidString
                self.verifyingKeys[keyId] = key
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("createVerifyingKey") { (hex: String) throws -> String in
            do {
                let key = try createVerifyingKey(hex: hex)
                let keyId = UUID().uuidString
                self.verifyingKeys[keyId] = key
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("verifyingKeyAddress") { (keyId: String) throws -> String in
            guard let key = self.verifyingKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("SignatureVerifyingKey not found")
            }
            return key.address()
        }

        Function("verifyingKeyToHex") { (keyId: String) throws -> String in
            guard let key = self.verifyingKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("SignatureVerifyingKey not found")
            }
            return key.toHex()
        }

        Function("verifyingKeyVerify") { (keyId: String, message: Data, signature: String) throws -> Bool in
            guard let key = self.verifyingKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("SignatureVerifyingKey not found")
            }
            do {
                return try key.verify(message: message, signature: signature)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("addressFromKey") { (keyId: String) throws -> String in
            guard let key = self.verifyingKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("SignatureVerifyingKey not found")
            }
            return addressFromKey(verifyingKey: key)
        }

        Function("disposeVerifyingKey") { (keyId: String) in
            self.verifyingKeys.removeValue(forKey: keyId)
        }

        // MARK: - Coin Operations

        Function("createShieldedCoinInfo") { (tokenType: String, value: Double) throws -> Data in
            do {
                let result = try createShieldedCoinInfo(tokenType: tokenType, value: UInt64(value))
                return result
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("coinCommitment") { (coinInfo: Data, coinPublicKey: String) throws -> String in
            do {
                return try coinCommitment(coinInfo: coinInfo, coinPublicKey: coinPublicKey)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("coinCommitmentFromFields") { (tokenType: String, nonce: String, value: Double, coinPublicKey: String) throws -> String in
            do {
                return try coinCommitmentFromFields(tokenType: tokenType, nonce: nonce, value: UInt64(value), coinPublicKey: coinPublicKey)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("coinNullifier") { (coinInfo: Data, coinSecretKeyId: String) throws -> String in
            guard let csk = self.coinSecretKeys[coinSecretKeyId] else {
                throw LedgerModuleError.invalidHandle("CoinSecretKey not found")
            }
            do {
                return try coinNullifier(coinInfo: coinInfo, coinSecretKey: csk)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("coinNullifierFromFields") { (tokenType: String, nonce: String, value: Double, coinSecretKeyId: String) throws -> String in
            guard let csk = self.coinSecretKeys[coinSecretKeyId] else {
                throw LedgerModuleError.invalidHandle("CoinSecretKey not found")
            }
            do {
                return try coinNullifierFromFields(tokenType: tokenType, nonce: nonce, value: UInt64(value), coinSecretKey: csk)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        // MARK: - Signing

        Function("signData") { (signingKey: String, payload: Data) throws -> String in
            do {
                return try signData(signingKey: signingKey, payload: payload)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("verifySignature") { (keyId: String, message: Data, signature: String) throws -> Bool in
            guard let key = self.verifyingKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("SignatureVerifyingKey not found")
            }
            do {
                return try verifySignature(verifyingKey: key, message: message, signature: signature)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        // MARK: - ZswapSecretKeys Management

        Function("createZswapSecretKeys") { (seed: Data) throws -> String in
            do {
                let keys = try ZswapSecretKeys.fromSeed(seed: seed)
                let keyId = UUID().uuidString
                self.zswapSecretKeys[keyId] = keys
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("getZswapCoinPublicKey") { (keyId: String) throws -> String in
            guard let keys = self.zswapSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            do {
                return try keys.coinPublicKey()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("getZswapEncryptionPublicKey") { (keyId: String) throws -> String in
            guard let keys = self.zswapSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            do {
                return try keys.encryptionPublicKey()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("getZswapCoinSecretKey") { (keyId: String) throws -> String in
            guard let keys = self.zswapSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            do {
                let csk = try keys.coinSecretKey()
                let cskId = UUID().uuidString
                self.coinSecretKeys[cskId] = csk
                return cskId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("getZswapEncryptionSecretKey") { (keyId: String) throws -> String in
            guard let keys = self.zswapSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            do {
                let esk = try keys.encryptionSecretKey()
                let eskId = UUID().uuidString
                self.encryptionSecretKeys[eskId] = esk
                return eskId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("clearZswapSecretKeys") { (keyId: String) in
            if let keys = self.zswapSecretKeys[keyId] {
                keys.clear()
                self.zswapSecretKeys.removeValue(forKey: keyId)
            }
        }

        // MARK: - CoinSecretKey Management

        Function("coinSecretKeyPublicKey") { (keyId: String) throws -> String in
            guard let csk = self.coinSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("CoinSecretKey not found")
            }
            do {
                return try csk.publicKey()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("coinSecretKeySerialize") { (keyId: String) throws -> Data in
            guard let csk = self.coinSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("CoinSecretKey not found")
            }
            do {
                return try csk.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("clearCoinSecretKey") { (keyId: String) in
            if let csk = self.coinSecretKeys[keyId] {
                csk.clear()
                self.coinSecretKeys.removeValue(forKey: keyId)
            }
        }

        // MARK: - EncryptionSecretKey Management

        Function("deserializeEncryptionSecretKey") { (raw: Data) throws -> String in
            do {
                let esk = try EncryptionSecretKey.deserialize(raw: raw)
                let eskId = UUID().uuidString
                self.encryptionSecretKeys[eskId] = esk
                return eskId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("encryptionSecretKeyPublicKey") { (keyId: String) throws -> String in
            guard let esk = self.encryptionSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("EncryptionSecretKey not found")
            }
            do {
                return try esk.publicKey()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("encryptionSecretKeySerialize") { (keyId: String) throws -> Data in
            guard let esk = self.encryptionSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("EncryptionSecretKey not found")
            }
            do {
                return try esk.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("clearEncryptionSecretKey") { (keyId: String) in
            if let esk = self.encryptionSecretKeys[keyId] {
                esk.clear()
                self.encryptionSecretKeys.removeValue(forKey: keyId)
            }
        }

        // MARK: - DustSecretKey Management

        Function("createDustSecretKey") { (seed: Data) throws -> String in
            do {
                let key = try DustSecretKey.fromSeed(seed: seed)
                let keyId = UUID().uuidString
                self.dustSecretKeys[keyId] = key
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("getDustPublicKey") { (keyId: String) throws -> String in
            guard let key = self.dustSecretKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("DustSecretKey not found")
            }
            do {
                return try key.publicKey()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("clearDustSecretKey") { (keyId: String) in
            if let key = self.dustSecretKeys[keyId] {
                key.clear()
                self.dustSecretKeys.removeValue(forKey: keyId)
            }
        }

        // MARK: - Transaction Operations

        Function("createTransaction") { (networkId: String, intentId: String?) throws -> String in
            do {
                var intent: Intent? = nil
                if let iId = intentId {
                    intent = self.intents[iId]
                }
                let tx = try createTransaction(networkId: networkId, intent: intent)
                let txId = UUID().uuidString
                self.transactions[txId] = tx
                return txId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("createTransactionRandomized") { (networkId: String, intentId: String?) throws -> String in
            do {
                var intent: Intent? = nil
                if let iId = intentId {
                    intent = self.intents[iId]
                }
                let tx = try createTransactionRandomized(networkId: networkId, intent: intent)
                let txId = UUID().uuidString
                self.transactions[txId] = tx
                return txId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeTransaction") { (data: Data) throws -> String in
            do {
                let tx = try deserializeTransaction(raw: data)
                let txId = UUID().uuidString
                self.transactions[txId] = tx
                return txId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeTransactionTyped") { (signatureMarker: String, proofMarker: String, bindingMarker: String, data: Data) throws -> String in
            do {
                let tx = try deserializeTransactionTyped(
                    signatureMarker: signatureMarker,
                    proofMarker: proofMarker,
                    bindingMarker: bindingMarker,
                    raw: data
                )
                let txId = UUID().uuidString
                self.transactions[txId] = tx
                return txId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("transactionNetworkId") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            return tx.networkId()
        }

        Function("bindTransaction") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            do {
                let boundTx = try tx.bind()
                let newTxId = UUID().uuidString
                self.transactions[newTxId] = boundTx
                return newTxId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("mockProveTransaction") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            do {
                let provenTx = try tx.mockProve()
                let newTxId = UUID().uuidString
                self.transactions[newTxId] = provenTx
                return newTxId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("mergeTransactions") { (txId1: String, txId2: String) throws -> String in
            guard let tx1 = self.transactions[txId1] else {
                throw LedgerModuleError.invalidHandle("Transaction 1 not found")
            }
            guard let tx2 = self.transactions[txId2] else {
                throw LedgerModuleError.invalidHandle("Transaction 2 not found")
            }
            do {
                let merged = try tx1.merge(other: tx2)
                let newTxId = UUID().uuidString
                self.transactions[newTxId] = merged
                return newTxId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("transactionIdentifiers") { (txId: String) throws -> [String] in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            do {
                return try tx.identifiers()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("eraseTransactionProofs") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            let erasedTx = tx.eraseProofs()
            let newTxId = UUID().uuidString
            self.transactions[newTxId] = erasedTx
            return newTxId
        }

        Function("eraseTransactionSignatures") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            let erasedTx = tx.eraseSignatures()
            let newTxId = UUID().uuidString
            self.transactions[newTxId] = erasedTx
            return newTxId
        }

        Function("serializeTransaction") { (txId: String) throws -> Data in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            do {
                return try tx.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("transactionToDebugString") { (txId: String) throws -> String in
            guard let tx = self.transactions[txId] else {
                throw LedgerModuleError.invalidHandle("Transaction not found")
            }
            return tx.toDebugString()
        }

        Function("disposeTransaction") { (txId: String) in
            self.transactions.removeValue(forKey: txId)
        }

        // MARK: - Intent Operations

        Function("createIntent") { (ttlSeconds: Double) -> String in
            let intent = createIntent(ttlSeconds: UInt64(ttlSeconds))
            let intentId = UUID().uuidString
            self.intents[intentId] = intent
            return intentId
        }

        Function("deserializeIntent") { (data: Data) throws -> String in
            do {
                let intent = try deserializeIntent(raw: data)
                let intentId = UUID().uuidString
                self.intents[intentId] = intent
                return intentId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("intentTtlSeconds") { (intentId: String) throws -> Double in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            return Double(intent.ttlSeconds())
        }

        Function("intentSetTtl") { (intentId: String, ttlSeconds: Double) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            let newIntent = intent.setTtl(ttlSeconds: UInt64(ttlSeconds))
            let newIntentId = UUID().uuidString
            self.intents[newIntentId] = newIntent
            return newIntentId
        }

        Function("intentSignatureData") { (intentId: String, segmentId: Int) throws -> Data in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            return intent.signatureData(segmentId: UInt16(segmentId))
        }

        Function("intentIntentHash") { (intentId: String, segmentId: Int) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            return intent.intentHash(segmentId: UInt16(segmentId))
        }

        Function("intentGuaranteedUnshieldedOffer") { (intentId: String) throws -> String? in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            if let offer = intent.guaranteedUnshieldedOffer() {
                let offerId = UUID().uuidString
                self.unshieldedOffers[offerId] = offer
                return offerId
            }
            return nil
        }

        Function("intentSetGuaranteedUnshieldedOffer") { (intentId: String, offerId: String?) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            var offer: UnshieldedOffer? = nil
            if let oId = offerId {
                offer = self.unshieldedOffers[oId]
            }
            do {
                let newIntent = try intent.setGuaranteedUnshieldedOffer(offer: offer)
                let newIntentId = UUID().uuidString
                self.intents[newIntentId] = newIntent
                return newIntentId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("intentFallibleUnshieldedOffer") { (intentId: String) throws -> String? in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            if let offer = intent.fallibleUnshieldedOffer() {
                let offerId = UUID().uuidString
                self.unshieldedOffers[offerId] = offer
                return offerId
            }
            return nil
        }

        Function("intentSetFallibleUnshieldedOffer") { (intentId: String, offerId: String?) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            var offer: UnshieldedOffer? = nil
            if let oId = offerId {
                offer = self.unshieldedOffers[oId]
            }
            do {
                let newIntent = try intent.setFallibleUnshieldedOffer(offer: offer)
                let newIntentId = UUID().uuidString
                self.intents[newIntentId] = newIntent
                return newIntentId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("intentBind") { (intentId: String, segmentId: Int) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            do {
                let boundIntent = try intent.bind(segmentId: UInt16(segmentId))
                let newIntentId = UUID().uuidString
                self.intents[newIntentId] = boundIntent
                return newIntentId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("serializeIntent") { (intentId: String) throws -> Data in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            do {
                return try intent.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("intentToDebugString") { (intentId: String) throws -> String in
            guard let intent = self.intents[intentId] else {
                throw LedgerModuleError.invalidHandle("Intent not found")
            }
            return intent.toDebugString()
        }

        Function("disposeIntent") { (intentId: String) in
            self.intents.removeValue(forKey: intentId)
        }

        // MARK: - UnshieldedOffer Operations

        Function("createUnshieldedOffer") { (inputIds: [String], outputIds: [String], signatures: [String]) throws -> String in
            var inputs: [UtxoSpend] = []
            for inputId in inputIds {
                guard let spend = self.utxoSpends[inputId] else {
                    throw LedgerModuleError.invalidHandle("UtxoSpend not found: \(inputId)")
                }
                inputs.append(spend)
            }
            var outputs: [UtxoOutput] = []
            for outputId in outputIds {
                guard let output = self.utxoOutputs[outputId] else {
                    throw LedgerModuleError.invalidHandle("UtxoOutput not found: \(outputId)")
                }
                outputs.append(output)
            }
            do {
                let offer = try createUnshieldedOffer(inputs: inputs, outputs: outputs, signatures: signatures)
                let offerId = UUID().uuidString
                self.unshieldedOffers[offerId] = offer
                return offerId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("createUnshieldedOfferUnsigned") { (inputIds: [String], outputIds: [String]) throws -> String in
            var inputs: [UtxoSpend] = []
            for inputId in inputIds {
                guard let spend = self.utxoSpends[inputId] else {
                    throw LedgerModuleError.invalidHandle("UtxoSpend not found: \(inputId)")
                }
                inputs.append(spend)
            }
            var outputs: [UtxoOutput] = []
            for outputId in outputIds {
                guard let output = self.utxoOutputs[outputId] else {
                    throw LedgerModuleError.invalidHandle("UtxoOutput not found: \(outputId)")
                }
                outputs.append(output)
            }
            do {
                let offer = try createUnshieldedOfferUnsigned(inputs: inputs, outputs: outputs)
                let offerId = UUID().uuidString
                self.unshieldedOffers[offerId] = offer
                return offerId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeUnshieldedOffer") { (data: Data) throws -> String in
            do {
                let offer = try deserializeUnshieldedOffer(raw: data)
                let offerId = UUID().uuidString
                self.unshieldedOffers[offerId] = offer
                return offerId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("unshieldedOfferInputs") { (offerId: String) throws -> [String] in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            let inputs = offer.inputs()
            var inputIds: [String] = []
            for input in inputs {
                let inputId = UUID().uuidString
                self.utxoSpends[inputId] = input
                inputIds.append(inputId)
            }
            return inputIds
        }

        Function("unshieldedOfferOutputs") { (offerId: String) throws -> [String] in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            let outputs = offer.outputs()
            var outputIds: [String] = []
            for output in outputs {
                let outputId = UUID().uuidString
                self.utxoOutputs[outputId] = output
                outputIds.append(outputId)
            }
            return outputIds
        }

        Function("unshieldedOfferSignatures") { (offerId: String) throws -> [String] in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            return offer.signatures()
        }

        Function("unshieldedOfferAddSignatures") { (offerId: String, signatures: [String]) throws -> String in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            do {
                let newOffer = try offer.addSignatures(signatures: signatures)
                let newOfferId = UUID().uuidString
                self.unshieldedOffers[newOfferId] = newOffer
                return newOfferId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("serializeUnshieldedOffer") { (offerId: String) throws -> Data in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            do {
                return try offer.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("unshieldedOfferToDebugString") { (offerId: String) throws -> String in
            guard let offer = self.unshieldedOffers[offerId] else {
                throw LedgerModuleError.invalidHandle("UnshieldedOffer not found")
            }
            return offer.toDebugString()
        }

        Function("disposeUnshieldedOffer") { (offerId: String) in
            self.unshieldedOffers.removeValue(forKey: offerId)
        }

        // MARK: - UtxoSpend Operations

        Function("createUtxoSpend") { (value: String, owner: String, tokenType: String, intentHash: String, outputNo: Int) throws -> String in
            do {
                let spend = try createUtxoSpend(
                    value: value,
                    owner: owner,
                    tokenType: tokenType,
                    intentHash: intentHash,
                    outputNo: UInt32(outputNo)
                )
                let spendId = UUID().uuidString
                self.utxoSpends[spendId] = spend
                return spendId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeUtxoSpend") { (data: Data) throws -> String in
            do {
                let spend = try deserializeUtxoSpend(raw: data)
                let spendId = UUID().uuidString
                self.utxoSpends[spendId] = spend
                return spendId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("utxoSpendValue") { (spendId: String) throws -> String in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            return spend.value()
        }

        Function("utxoSpendOwner") { (spendId: String) throws -> String in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            return spend.owner()
        }

        Function("utxoSpendTokenType") { (spendId: String) throws -> String in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            return spend.tokenType()
        }

        Function("utxoSpendIntentHash") { (spendId: String) throws -> String in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            return spend.intentHash()
        }

        Function("utxoSpendOutputNo") { (spendId: String) throws -> Int in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            return Int(spend.outputNo())
        }

        Function("serializeUtxoSpend") { (spendId: String) throws -> Data in
            guard let spend = self.utxoSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("UtxoSpend not found")
            }
            do {
                return try spend.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeUtxoSpend") { (spendId: String) in
            self.utxoSpends.removeValue(forKey: spendId)
        }

        // MARK: - UtxoOutput Operations

        Function("createUtxoOutput") { (value: String, owner: String, tokenType: String) throws -> String in
            do {
                let output = try createUtxoOutput(value: value, owner: owner, tokenType: tokenType)
                let outputId = UUID().uuidString
                self.utxoOutputs[outputId] = output
                return outputId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeUtxoOutput") { (data: Data) throws -> String in
            do {
                let output = try deserializeUtxoOutput(raw: data)
                let outputId = UUID().uuidString
                self.utxoOutputs[outputId] = output
                return outputId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("utxoOutputValue") { (outputId: String) throws -> String in
            guard let output = self.utxoOutputs[outputId] else {
                throw LedgerModuleError.invalidHandle("UtxoOutput not found")
            }
            return output.value()
        }

        Function("utxoOutputOwner") { (outputId: String) throws -> String in
            guard let output = self.utxoOutputs[outputId] else {
                throw LedgerModuleError.invalidHandle("UtxoOutput not found")
            }
            return output.owner()
        }

        Function("utxoOutputTokenType") { (outputId: String) throws -> String in
            guard let output = self.utxoOutputs[outputId] else {
                throw LedgerModuleError.invalidHandle("UtxoOutput not found")
            }
            return output.tokenType()
        }

        Function("serializeUtxoOutput") { (outputId: String) throws -> Data in
            guard let output = self.utxoOutputs[outputId] else {
                throw LedgerModuleError.invalidHandle("UtxoOutput not found")
            }
            do {
                return try output.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeUtxoOutput") { (outputId: String) in
            self.utxoOutputs.removeValue(forKey: outputId)
        }

        // MARK: - ZswapLocalState Operations

        Function("createZswapLocalState") { () -> String in
            let state = ZswapLocalState()
            let stateId = UUID().uuidString
            self.zswapLocalStates[stateId] = state
            return stateId
        }

        Function("deserializeZswapLocalState") { (data: Data) throws -> String in
            do {
                let state = try deserializeZswapLocalState(raw: data)
                let stateId = UUID().uuidString
                self.zswapLocalStates[stateId] = state
                return stateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateFirstFree") { (stateId: String) throws -> Double in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            return Double(state.firstFree())
        }

        Function("zswapLocalStateCoinsCount") { (stateId: String) throws -> Double in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            return Double(state.coinsCount())
        }

        Function("zswapLocalStateCoins") { (stateId: String) throws -> [String] in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            do {
                return try state.coins()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateCoinsData") { (stateId: String) throws -> [[String: Any]] in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            let coins = state.coinsData()
            return coins.map { coin in
                return [
                    "type": coin.tokenType(),
                    "nonce": coin.nonce(),
                    "value": coin.value(),
                    "mt_index": String(coin.mtIndex())
                ] as [String: Any]
            }
        }

        Function("zswapLocalStatePendingSpendsData") { (stateId: String) throws -> [[String: Any]] in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            let pendingSpends = state.pendingSpendsData()
            return pendingSpends.map { entry in
                let coin = entry.coin()
                return [
                    "nullifier": entry.nullifier(),
                    "type": coin.tokenType(),
                    "nonce": coin.nonce(),
                    "value": coin.value(),
                    "mt_index": String(coin.mtIndex())
                ] as [String: Any]
            }
        }

        Function("zswapLocalStatePendingOutputsData") { (stateId: String) throws -> [[String: Any]] in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            let pendingOutputs = state.pendingOutputsData()
            return pendingOutputs.map { entry in
                let coin = entry.coin()
                return [
                    "commitment": entry.commitment(),
                    "type": coin.tokenType(),
                    "nonce": coin.nonce(),
                    "value": coin.value()
                ] as [String: Any]
            }
        }

        Function("zswapLocalStateReplayEvents") { (stateId: String, secretKeysId: String, eventIds: [String]) throws -> String in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            guard let keys = self.zswapSecretKeys[secretKeysId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            var events: [Event] = []
            for eventId in eventIds {
                guard let event = self.events[eventId] else {
                    throw LedgerModuleError.invalidHandle("Event not found: \(eventId)")
                }
                events.append(event)
            }
            do {
                let newState = try state.replayEvents(secretKeys: keys, events: events)
                let newStateId = UUID().uuidString
                self.zswapLocalStates[newStateId] = newState
                return newStateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateApplyCollapsedUpdate") { (stateId: String, updateId: String) throws -> String in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            guard let update = self.merkleUpdates[updateId] else {
                throw LedgerModuleError.invalidHandle("MerkleTreeCollapsedUpdate not found")
            }
            do {
                let newState = try state.applyCollapsedUpdate(update: update)
                let newStateId = UUID().uuidString
                self.zswapLocalStates[newStateId] = newState
                return newStateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateWatchFor") { (stateId: String, coinPublicKey: String, coinInfo: Data) throws -> String in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            do {
                let newState = try state.watchFor(coinPublicKey: coinPublicKey, coinInfo: coinInfo)
                let newStateId = UUID().uuidString
                self.zswapLocalStates[newStateId] = newState
                return newStateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateSpend") { (stateId: String, secretKeysId: String, coin: Data, segment: Int?) throws -> [String: String] in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            guard let keys = self.zswapSecretKeys[secretKeysId] else {
                throw LedgerModuleError.invalidHandle("ZswapSecretKeys not found")
            }
            do {
                let segmentValue: UInt16? = segment.map { UInt16($0) }
                let result = try state.spend(secretKeys: keys, coin: coin, segment: segmentValue)
                let newStateId = UUID().uuidString
                let inputId = UUID().uuidString
                self.zswapLocalStates[newStateId] = result.state()
                self.zswapInputs[inputId] = result.input()
                return ["stateId": newStateId, "inputId": inputId]
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        // ZswapInput operations
        Function("zswapInputNullifier") { (inputId: String) throws -> String in
            guard let input = self.zswapInputs[inputId] else {
                throw LedgerModuleError.invalidHandle("ZswapInput not found")
            }
            return input.nullifier()
        }

        Function("zswapInputContractAddress") { (inputId: String) throws -> String? in
            guard let input = self.zswapInputs[inputId] else {
                throw LedgerModuleError.invalidHandle("ZswapInput not found")
            }
            return input.contractAddress()
        }

        Function("serializeZswapInput") { (inputId: String) throws -> Data in
            guard let input = self.zswapInputs[inputId] else {
                throw LedgerModuleError.invalidHandle("ZswapInput not found")
            }
            do {
                return try input.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapInputToDebugString") { (inputId: String) throws -> String in
            guard let input = self.zswapInputs[inputId] else {
                throw LedgerModuleError.invalidHandle("ZswapInput not found")
            }
            return input.toDebugString()
        }

        Function("disposeZswapInput") { (inputId: String) in
            self.zswapInputs.removeValue(forKey: inputId)
        }

        Function("serializeZswapLocalState") { (stateId: String) throws -> Data in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            do {
                return try state.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("zswapLocalStateToDebugString") { (stateId: String) throws -> String in
            guard let state = self.zswapLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("ZswapLocalState not found")
            }
            return state.toDebugString()
        }

        Function("disposeZswapLocalState") { (stateId: String) in
            self.zswapLocalStates.removeValue(forKey: stateId)
        }

        // MARK: - DustLocalState Operations

        Function("createDustLocalState") { (paramsId: String) throws -> String in
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            let state = DustLocalState(params: params)
            let stateId = UUID().uuidString
            self.dustLocalStates[stateId] = state
            return stateId
        }

        Function("deserializeDustLocalState") { (data: Data) throws -> String in
            do {
                let state = try deserializeDustLocalState(raw: data)
                let stateId = UUID().uuidString
                self.dustLocalStates[stateId] = state
                return stateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustLocalStateWalletBalance") { (stateId: String, timeSeconds: Double) throws -> String in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            return state.walletBalance(timeSeconds: UInt64(timeSeconds))
        }

        Function("dustLocalStateSyncTimeSeconds") { (stateId: String) throws -> Double in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            return Double(state.syncTimeSeconds())
        }

        Function("dustLocalStateUtxosCount") { (stateId: String) throws -> Double in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            return Double(state.utxosCount())
        }

        Function("dustLocalStateUtxos") { (stateId: String) throws -> [[String: Any]] in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            let utxos = state.utxos()
            return utxos.map { utxo in
                // Store the utxo and return its ID along with data
                let utxoId = UUID().uuidString
                self.qualifiedDustOutputs[utxoId] = utxo
                return [
                    "id": utxoId,
                    "nonce": utxo.nonce(),
                    "initialValue": utxo.initialValue(),
                    "mtIndex": String(utxo.mtIndex()),
                    "ctimeSeconds": utxo.ctimeSeconds(),
                    "seq": utxo.seq(),
                    "owner": utxo.owner().toHex(),
                    "backingNight": utxo.backingNight().toHex()
                ] as [String: Any]
            }
        }

        Function("disposeQualifiedDustOutput") { (utxoId: String) in
            self.qualifiedDustOutputs.removeValue(forKey: utxoId)
        }

        Function("dustLocalStateParams") { (stateId: String) throws -> String in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            let params = state.params()
            let paramsId = UUID().uuidString
            self.dustParameters[paramsId] = params
            return paramsId
        }

        Function("dustLocalStateProcessTtls") { (stateId: String, timeSeconds: Double) throws -> String in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            let newState = state.processTtls(timeSeconds: UInt64(timeSeconds))
            let newStateId = UUID().uuidString
            self.dustLocalStates[newStateId] = newState
            return newStateId
        }

        Function("dustLocalStateReplayEvents") { (stateId: String, secretKeyId: String, eventIds: [String]) throws -> String in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            guard let key = self.dustSecretKeys[secretKeyId] else {
                throw LedgerModuleError.invalidHandle("DustSecretKey not found")
            }
            var events: [Event] = []
            for eventId in eventIds {
                guard let event = self.events[eventId] else {
                    throw LedgerModuleError.invalidHandle("Event not found: \(eventId)")
                }
                events.append(event)
            }
            do {
                let newState = try state.replayEvents(secretKey: key, events: events)
                let newStateId = UUID().uuidString
                self.dustLocalStates[newStateId] = newState
                return newStateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustLocalStateSpend") { (stateId: String, secretKeyId: String, utxoId: String, vFee: String, ctimeSeconds: Double) throws -> [String: String] in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            guard let key = self.dustSecretKeys[secretKeyId] else {
                throw LedgerModuleError.invalidHandle("DustSecretKey not found")
            }
            guard let utxo = self.qualifiedDustOutputs[utxoId] else {
                throw LedgerModuleError.invalidHandle("QualifiedDustOutput not found")
            }
            do {
                let result = try state.spend(secretKey: key, utxo: utxo, vFee: vFee, ctimeSeconds: UInt64(ctimeSeconds))
                let newStateId = UUID().uuidString
                let spendId = UUID().uuidString
                self.dustLocalStates[newStateId] = result.state()
                self.dustSpends[spendId] = result.spend()
                return ["stateId": newStateId, "spendId": spendId]
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        // DustSpend operations
        Function("dustSpendVFee") { (spendId: String) throws -> String in
            guard let spend = self.dustSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("DustSpend not found")
            }
            return spend.vFee()
        }

        Function("dustSpendOldNullifier") { (spendId: String) throws -> String in
            guard let spend = self.dustSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("DustSpend not found")
            }
            return spend.oldNullifier()
        }

        Function("dustSpendNewCommitment") { (spendId: String) throws -> String in
            guard let spend = self.dustSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("DustSpend not found")
            }
            return spend.newCommitment()
        }

        Function("serializeDustSpend") { (spendId: String) throws -> Data in
            guard let spend = self.dustSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("DustSpend not found")
            }
            do {
                return try spend.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustSpendToDebugString") { (spendId: String) throws -> String in
            guard let spend = self.dustSpends[spendId] else {
                throw LedgerModuleError.invalidHandle("DustSpend not found")
            }
            return spend.toDebugString()
        }

        Function("disposeDustSpend") { (spendId: String) in
            self.dustSpends.removeValue(forKey: spendId)
        }

        Function("serializeDustLocalState") { (stateId: String) throws -> Data in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            do {
                return try state.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustLocalStateToDebugString") { (stateId: String) throws -> String in
            guard let state = self.dustLocalStates[stateId] else {
                throw LedgerModuleError.invalidHandle("DustLocalState not found")
            }
            return state.toDebugString()
        }

        Function("disposeDustLocalState") { (stateId: String) in
            self.dustLocalStates.removeValue(forKey: stateId)
        }

        // MARK: - LedgerState Operations

        Function("createLedgerState") { (networkId: String) -> String in
            let state = LedgerState.blank(networkId: networkId)
            let stateId = UUID().uuidString
            self.ledgerStates[stateId] = state
            return stateId
        }

        Function("deserializeLedgerState") { (data: Data) throws -> String in
            do {
                let state = try deserializeLedgerState(raw: data)
                let stateId = UUID().uuidString
                self.ledgerStates[stateId] = state
                return stateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("ledgerStateNetworkId") { (stateId: String) throws -> String in
            guard let state = self.ledgerStates[stateId] else {
                throw LedgerModuleError.invalidHandle("LedgerState not found")
            }
            return state.networkId()
        }

        Function("serializeLedgerState") { (stateId: String) throws -> Data in
            guard let state = self.ledgerStates[stateId] else {
                throw LedgerModuleError.invalidHandle("LedgerState not found")
            }
            do {
                return try state.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("ledgerStateToDebugString") { (stateId: String) throws -> String in
            guard let state = self.ledgerStates[stateId] else {
                throw LedgerModuleError.invalidHandle("LedgerState not found")
            }
            return state.toDebugString()
        }

        Function("disposeLedgerState") { (stateId: String) in
            self.ledgerStates.removeValue(forKey: stateId)
        }

        // MARK: - Event Operations

        Function("deserializeEvent") { (data: Data) throws -> String in
            do {
                let event = try deserializeEvent(raw: data)
                let eventId = UUID().uuidString
                self.events[eventId] = event
                return eventId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("eventType") { (eventId: String) throws -> String in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.eventType()
        }

        Function("eventIsZswapEvent") { (eventId: String) throws -> Bool in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.isZswapEvent()
        }

        Function("eventIsDustEvent") { (eventId: String) throws -> Bool in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.isDustEvent()
        }

        Function("eventIsContractEvent") { (eventId: String) throws -> Bool in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.isContractEvent()
        }

        Function("eventIsParamChangeEvent") { (eventId: String) throws -> Bool in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.isParamChangeEvent()
        }

        Function("eventZswapInputNullifier") { (eventId: String) throws -> String? in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.zswapInputNullifier()
        }

        Function("eventZswapOutputCommitment") { (eventId: String) throws -> String? in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.zswapOutputCommitment()
        }

        Function("eventZswapOutputMtIndex") { (eventId: String) throws -> Double? in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            if let index = event.zswapOutputMtIndex() {
                return Double(index)
            }
            return nil
        }

        Function("serializeEvent") { (eventId: String) throws -> Data in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            do {
                return try event.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("eventToDebugString") { (eventId: String) throws -> String in
            guard let event = self.events[eventId] else {
                throw LedgerModuleError.invalidHandle("Event not found")
            }
            return event.toDebugString()
        }

        Function("disposeEvent") { (eventId: String) in
            self.events.removeValue(forKey: eventId)
        }

        // MARK: - MerkleTreeCollapsedUpdate Operations

        Function("deserializeMerkleTreeCollapsedUpdate") { (data: Data) throws -> String in
            do {
                let update = try deserializeMerkleTreeCollapsedUpdate(raw: data)
                let updateId = UUID().uuidString
                self.merkleUpdates[updateId] = update
                return updateId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("serializeMerkleTreeCollapsedUpdate") { (updateId: String) throws -> Data in
            guard let update = self.merkleUpdates[updateId] else {
                throw LedgerModuleError.invalidHandle("MerkleTreeCollapsedUpdate not found")
            }
            do {
                return try update.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeMerkleTreeCollapsedUpdate") { (updateId: String) in
            self.merkleUpdates.removeValue(forKey: updateId)
        }

        // MARK: - DustParameters Operations

        Function("createDustParameters") { (nightDustRatio: Double, generationDecayRate: Int, dustGracePeriodSeconds: Double) -> String in
            let params = DustParameters(
                nightDustRatio: UInt64(nightDustRatio),
                generationDecayRate: UInt32(generationDecayRate),
                dustGracePeriodSeconds: Int64(dustGracePeriodSeconds)
            )
            let paramsId = UUID().uuidString
            self.dustParameters[paramsId] = params
            return paramsId
        }

        Function("deserializeDustParameters") { (data: Data) throws -> String in
            do {
                let params = try deserializeDustParameters(raw: data)
                let paramsId = UUID().uuidString
                self.dustParameters[paramsId] = params
                return paramsId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustParametersNightDustRatio") { (paramsId: String) throws -> Double in
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            return Double(params.nightDustRatio())
        }

        Function("dustParametersGenerationDecayRate") { (paramsId: String) throws -> Int in
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            return Int(params.generationDecayRate())
        }

        Function("dustParametersDustGracePeriodSeconds") { (paramsId: String) throws -> Double in
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            return Double(params.dustGracePeriodSeconds())
        }

        Function("serializeDustParameters") { (paramsId: String) throws -> Data in
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            do {
                return try params.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeDustParameters") { (paramsId: String) in
            self.dustParameters.removeValue(forKey: paramsId)
        }

        // MARK: - LedgerParameters Operations

        Function("initialLedgerParameters") { () -> String in
            let params = initialLedgerParameters()
            let paramsId = UUID().uuidString
            self.ledgerParameters[paramsId] = params
            return paramsId
        }

        Function("deserializeLedgerParameters") { (data: Data) throws -> String in
            do {
                let params = try deserializeLedgerParameters(raw: data)
                let paramsId = UUID().uuidString
                self.ledgerParameters[paramsId] = params
                return paramsId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("ledgerParametersDustParams") { (paramsId: String) throws -> String in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            let dustParams = params.dustParams()
            let dustParamsId = UUID().uuidString
            self.dustParameters[dustParamsId] = dustParams
            return dustParamsId
        }

        Function("ledgerParametersGlobalTtlSeconds") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return Double(params.globalTtlSeconds())
        }

        Function("ledgerParametersTransactionByteLimit") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return Double(params.transactionByteLimit())
        }

        Function("ledgerParametersCardanoBridgeFeeBasisPoints") { (paramsId: String) throws -> Int in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return Int(params.cardanoBridgeFeeBasisPoints())
        }

        Function("ledgerParametersCardanoBridgeMinAmount") { (paramsId: String) throws -> String in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.cardanoBridgeMinAmount()
        }

        Function("ledgerParametersFeeOverallPrice") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.feeOverallPrice()
        }

        Function("ledgerParametersFeeReadFactor") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.feeReadFactor()
        }

        Function("ledgerParametersFeeComputeFactor") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.feeComputeFactor()
        }

        Function("ledgerParametersFeeBlockUsageFactor") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.feeBlockUsageFactor()
        }

        Function("ledgerParametersFeeWriteFactor") { (paramsId: String) throws -> Double in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.feeWriteFactor()
        }

        Function("ledgerParametersMinClaimableRewards") { (paramsId: String) throws -> String in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.minClaimableRewards()
        }

        Function("serializeLedgerParameters") { (paramsId: String) throws -> Data in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            do {
                return try params.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("ledgerParametersToDebugString") { (paramsId: String) throws -> String in
            guard let params = self.ledgerParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("LedgerParameters not found")
            }
            return params.toDebugString()
        }

        Function("disposeLedgerParameters") { (paramsId: String) in
            self.ledgerParameters.removeValue(forKey: paramsId)
        }

        // MARK: - BlockContext Operations

        Function("createBlockContext") { (tblockSeconds: Double) -> String in
            let ctx = createBlockContext(tblockSeconds: UInt64(tblockSeconds))
            let ctxId = UUID().uuidString
            self.blockContexts[ctxId] = ctx
            return ctxId
        }

        Function("createBlockContextFull") { (tblockSeconds: Double, tblockErr: Int, parentBlockHash: String) throws -> String in
            do {
                let ctx = try createBlockContextFull(
                    tblockSeconds: UInt64(tblockSeconds),
                    tblockErr: UInt32(tblockErr),
                    parentBlockHash: parentBlockHash
                )
                let ctxId = UUID().uuidString
                self.blockContexts[ctxId] = ctx
                return ctxId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeBlockContext") { (data: Data) throws -> String in
            do {
                let ctx = try deserializeBlockContext(raw: data)
                let ctxId = UUID().uuidString
                self.blockContexts[ctxId] = ctx
                return ctxId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("blockContextTblockSeconds") { (ctxId: String) throws -> Double in
            guard let ctx = self.blockContexts[ctxId] else {
                throw LedgerModuleError.invalidHandle("BlockContext not found")
            }
            return Double(ctx.tblockSeconds())
        }

        Function("blockContextTblockErr") { (ctxId: String) throws -> Int in
            guard let ctx = self.blockContexts[ctxId] else {
                throw LedgerModuleError.invalidHandle("BlockContext not found")
            }
            return Int(ctx.tblockErr())
        }

        Function("blockContextParentBlockHash") { (ctxId: String) throws -> String in
            guard let ctx = self.blockContexts[ctxId] else {
                throw LedgerModuleError.invalidHandle("BlockContext not found")
            }
            return ctx.parentBlockHash()
        }

        Function("serializeBlockContext") { (ctxId: String) throws -> Data in
            guard let ctx = self.blockContexts[ctxId] else {
                throw LedgerModuleError.invalidHandle("BlockContext not found")
            }
            do {
                return try ctx.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("blockContextToDebugString") { (ctxId: String) throws -> String in
            guard let ctx = self.blockContexts[ctxId] else {
                throw LedgerModuleError.invalidHandle("BlockContext not found")
            }
            return ctx.toDebugString()
        }

        Function("disposeBlockContext") { (ctxId: String) in
            self.blockContexts.removeValue(forKey: ctxId)
        }

        // MARK: - ContractAddress Operations

        Function("createContractAddress") { (hex: String) throws -> String in
            do {
                let addr = try createContractAddress(hex: hex)
                let addrId = UUID().uuidString
                self.contractAddresses[addrId] = addr
                return addrId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeContractAddress") { (data: Data) throws -> String in
            do {
                let addr = try deserializeContractAddress(raw: data)
                let addrId = UUID().uuidString
                self.contractAddresses[addrId] = addr
                return addrId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("contractAddressToHex") { (addrId: String) throws -> String in
            guard let addr = self.contractAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("ContractAddress not found")
            }
            return addr.toHex()
        }

        Function("contractAddressCustomShieldedToken") { (addrId: String, domainSep: String) throws -> String in
            guard let addr = self.contractAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("ContractAddress not found")
            }
            do {
                return try addr.customShieldedToken(domainSep: domainSep)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("contractAddressCustomUnshieldedToken") { (addrId: String, domainSep: String) throws -> String in
            guard let addr = self.contractAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("ContractAddress not found")
            }
            do {
                return try addr.customUnshieldedToken(domainSep: domainSep)
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("serializeContractAddress") { (addrId: String) throws -> Data in
            guard let addr = self.contractAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("ContractAddress not found")
            }
            do {
                return try addr.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeContractAddress") { (addrId: String) in
            self.contractAddresses.removeValue(forKey: addrId)
        }

        // MARK: - PublicAddress Operations

        Function("createPublicAddressContract") { (contractAddrId: String) throws -> String in
            guard let contractAddr = self.contractAddresses[contractAddrId] else {
                throw LedgerModuleError.invalidHandle("ContractAddress not found")
            }
            let addr = createPublicAddressContract(contract: contractAddr)
            let addrId = UUID().uuidString
            self.publicAddresses[addrId] = addr
            return addrId
        }

        Function("createPublicAddressUser") { (userAddress: String) throws -> String in
            do {
                let addr = try createPublicAddressUser(userAddress: userAddress)
                let addrId = UUID().uuidString
                self.publicAddresses[addrId] = addr
                return addrId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializePublicAddress") { (data: Data) throws -> String in
            do {
                let addr = try deserializePublicAddress(raw: data)
                let addrId = UUID().uuidString
                self.publicAddresses[addrId] = addr
                return addrId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("publicAddressIsContract") { (addrId: String) throws -> Bool in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            return addr.isContract()
        }

        Function("publicAddressIsUser") { (addrId: String) throws -> Bool in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            return addr.isUser()
        }

        Function("publicAddressContractAddress") { (addrId: String) throws -> String? in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            if let contractAddr = addr.contractAddress() {
                let contractAddrId = UUID().uuidString
                self.contractAddresses[contractAddrId] = contractAddr
                return contractAddrId
            }
            return nil
        }

        Function("publicAddressUserAddress") { (addrId: String) throws -> String? in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            return addr.userAddress()
        }

        Function("publicAddressToHex") { (addrId: String) throws -> String in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            return addr.toHex()
        }

        Function("serializePublicAddress") { (addrId: String) throws -> Data in
            guard let addr = self.publicAddresses[addrId] else {
                throw LedgerModuleError.invalidHandle("PublicAddress not found")
            }
            do {
                return try addr.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposePublicAddress") { (addrId: String) in
            self.publicAddresses.removeValue(forKey: addrId)
        }

        // MARK: - Dust Updated Value

        Function("dustUpdatedValue") { (initialValue: String, ctimeSeconds: Double, genInfoId: String, nowSeconds: Double, paramsId: String) throws -> String in
            guard let genInfo = self.dustGenerationInfos[genInfoId] else {
                throw LedgerModuleError.invalidHandle("DustGenerationInfo not found")
            }
            guard let params = self.dustParameters[paramsId] else {
                throw LedgerModuleError.invalidHandle("DustParameters not found")
            }
            do {
                return try dustUpdatedValue(
                    initialValue: initialValue,
                    ctimeSeconds: UInt64(ctimeSeconds),
                    genInfo: genInfo,
                    nowSeconds: UInt64(nowSeconds),
                    params: params
                )
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        // MARK: - DustGenerationInfo Operations

        Function("deserializeDustGenerationInfo") { (data: Data) throws -> String in
            do {
                let info = try deserializeDustGenerationInfo(raw: data)
                let infoId = UUID().uuidString
                self.dustGenerationInfos[infoId] = info
                return infoId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustGenerationInfoValue") { (infoId: String) throws -> String in
            guard let info = self.dustGenerationInfos[infoId] else {
                throw LedgerModuleError.invalidHandle("DustGenerationInfo not found")
            }
            return info.value()
        }

        Function("dustGenerationInfoDtimeSeconds") { (infoId: String) throws -> Double in
            guard let info = self.dustGenerationInfos[infoId] else {
                throw LedgerModuleError.invalidHandle("DustGenerationInfo not found")
            }
            return Double(info.dtimeSeconds())
        }

        Function("serializeDustGenerationInfo") { (infoId: String) throws -> Data in
            guard let info = self.dustGenerationInfos[infoId] else {
                throw LedgerModuleError.invalidHandle("DustGenerationInfo not found")
            }
            do {
                return try info.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeDustGenerationInfo") { (infoId: String) in
            self.dustGenerationInfos.removeValue(forKey: infoId)
        }

        // MARK: - DustPublicKey Operations

        Function("createDustPublicKeyFromHex") { (hex: String) throws -> String in
            do {
                let key = try createDustPublicKey(hex: hex)
                let keyId = UUID().uuidString
                self.dustPublicKeys[keyId] = key
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeDustPublicKey") { (data: Data) throws -> String in
            do {
                let key = try deserializeDustPublicKey(raw: data)
                let keyId = UUID().uuidString
                self.dustPublicKeys[keyId] = key
                return keyId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("dustPublicKeyToHex") { (keyId: String) throws -> String in
            guard let key = self.dustPublicKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("DustPublicKey not found")
            }
            return key.toHex()
        }

        Function("serializeDustPublicKey") { (keyId: String) throws -> Data in
            guard let key = self.dustPublicKeys[keyId] else {
                throw LedgerModuleError.invalidHandle("DustPublicKey not found")
            }
            do {
                return try key.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeDustPublicKey") { (keyId: String) in
            self.dustPublicKeys.removeValue(forKey: keyId)
        }

        // MARK: - InitialNonce Operations

        Function("createInitialNonce") { (hex: String) throws -> String in
            do {
                let nonce = try createInitialNonce(hex: hex)
                let nonceId = UUID().uuidString
                self.initialNonces[nonceId] = nonce
                return nonceId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("deserializeInitialNonce") { (data: Data) throws -> String in
            do {
                let nonce = try deserializeInitialNonce(raw: data)
                let nonceId = UUID().uuidString
                self.initialNonces[nonceId] = nonce
                return nonceId
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("initialNonceToHex") { (nonceId: String) throws -> String in
            guard let nonce = self.initialNonces[nonceId] else {
                throw LedgerModuleError.invalidHandle("InitialNonce not found")
            }
            return nonce.toHex()
        }

        Function("serializeInitialNonce") { (nonceId: String) throws -> Data in
            guard let nonce = self.initialNonces[nonceId] else {
                throw LedgerModuleError.invalidHandle("InitialNonce not found")
            }
            do {
                return try nonce.serialize()
            } catch {
                throw LedgerModuleError.nativeError(error.localizedDescription)
            }
        }

        Function("disposeInitialNonce") { (nonceId: String) in
            self.initialNonces.removeValue(forKey: nonceId)
        }

        // MARK: - Cleanup

        OnDestroy {
            // Clear all secret keys securely
            for (_, keys) in self.zswapSecretKeys {
                keys.clear()
            }
            self.zswapSecretKeys.removeAll()

            for (_, csk) in self.coinSecretKeys {
                csk.clear()
            }
            self.coinSecretKeys.removeAll()

            for (_, esk) in self.encryptionSecretKeys {
                esk.clear()
            }
            self.encryptionSecretKeys.removeAll()

            for (_, key) in self.dustSecretKeys {
                key.clear()
            }
            self.dustSecretKeys.removeAll()

            // Clear other state
            self.verifyingKeys.removeAll()
            self.transactions.removeAll()
            self.intents.removeAll()
            self.unshieldedOffers.removeAll()
            self.utxoSpends.removeAll()
            self.utxoOutputs.removeAll()
            self.zswapLocalStates.removeAll()
            self.dustLocalStates.removeAll()
            self.ledgerStates.removeAll()
            self.events.removeAll()
            self.merkleUpdates.removeAll()
            self.dustParameters.removeAll()
            self.ledgerParameters.removeAll()
            self.blockContexts.removeAll()
            self.contractAddresses.removeAll()
            self.publicAddresses.removeAll()
            self.dustGenerationInfos.removeAll()
            self.dustPublicKeys.removeAll()
            self.initialNonces.removeAll()
        }
    }

    // MARK: - Private Storage

    private var verifyingKeys: [String: SignatureVerifyingKey] = [:]
    private var zswapSecretKeys: [String: ZswapSecretKeys] = [:]
    private var coinSecretKeys: [String: CoinSecretKey] = [:]
    private var encryptionSecretKeys: [String: EncryptionSecretKey] = [:]
    private var dustSecretKeys: [String: DustSecretKey] = [:]
    private var transactions: [String: Transaction] = [:]
    private var intents: [String: Intent] = [:]
    private var unshieldedOffers: [String: UnshieldedOffer] = [:]
    private var utxoSpends: [String: UtxoSpend] = [:]
    private var utxoOutputs: [String: UtxoOutput] = [:]
    private var zswapLocalStates: [String: ZswapLocalState] = [:]
    private var dustLocalStates: [String: DustLocalState] = [:]
    private var ledgerStates: [String: LedgerState] = [:]
    private var events: [String: Event] = [:]
    private var merkleUpdates: [String: MerkleTreeCollapsedUpdate] = [:]
    private var dustParameters: [String: DustParameters] = [:]
    private var ledgerParameters: [String: LedgerParameters] = [:]
    private var blockContexts: [String: BlockContext] = [:]
    private var contractAddresses: [String: ContractAddress] = [:]
    private var publicAddresses: [String: PublicAddress] = [:]
    private var dustGenerationInfos: [String: DustGenerationInfo] = [:]
    private var dustPublicKeys: [String: DustPublicKey] = [:]
    private var initialNonces: [String: InitialNonce] = [:]
    private var zswapInputs: [String: ZswapInput] = [:]
    private var dustSpends: [String: DustSpend] = [:]
    private var qualifiedDustOutputs: [String: QualifiedDustOutput] = [:]
}

// MARK: - Error Types

enum LedgerModuleError: Error {
    case invalidHandle(String)
    case nativeError(String)
}

extension LedgerModuleError: CustomStringConvertible {
    var description: String {
        switch self {
        case .invalidHandle(let msg):
            return "Invalid handle: \(msg)"
        case .nativeError(let msg):
            return "Native error: \(msg)"
        }
    }
}
