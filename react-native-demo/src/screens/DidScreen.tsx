/**
 * DidScreen — DID CRUD operations UI.
 *
 * Operations: resolve, deploy, update (addAlsoKnownAs), deactivate.
 *
 * **Heads up**: the contract calls behind these buttons are
 * currently stubbed (see `useDid.ts` for the integration plan).
 * The UI shapes and state flows ARE production-shaped — when the
 * upstream-TS contract bridge lands in RN, only the hook needs
 * to change; the screen's render logic stays the same.
 */

import React, { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useDid } from "../hooks/useDid";
import { formatMs } from "../utils/format";

export default function DidScreen(): React.JSX.Element {
  const insets = useSafeAreaInsets();
  const { resolved, inFlight, lastResult, resolve, deploy, update, deactivate, clear } = useDid();

  const [resolveInput, setResolveInput] = useState("did:midnight:");
  const [akaInput, setAkaInput] = useState("");

  return (
    <ScrollView
      contentContainerStyle={[
        styles.container,
        { paddingBottom: insets.bottom + 24 },
      ]}
    >
      <Text style={styles.warningBanner}>
        ⚠ Contract calls are stubbed in this demo. The UI shapes and
        state flow are production-shaped; integrating the upstream
        TS contract bridge is its own subproject (see `useDid.ts`).
      </Text>

      {/* ─── Resolve ──────────────────────────────────────── */}
      <View style={styles.card}>
        <Text style={styles.cardHeader}>Resolve DID</Text>
        <TextInput
          value={resolveInput}
          onChangeText={setResolveInput}
          style={styles.input}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder="did:midnight:..."
          placeholderTextColor="#666"
        />
        <PrimaryButton
          label="Resolve"
          onPress={() => resolve(resolveInput)}
          inFlight={inFlight?.kind === "resolve"}
          disabled={inFlight !== null}
        />
      </View>

      {/* ─── Resolved document ─────────────────────────────── */}
      {resolved !== null && (
        <View style={styles.card}>
          <Text style={styles.cardHeader}>Document</Text>
          <KV k="DID" v={resolved.did} mono />
          <KV k="Public key" v={resolved.publicKey} mono />
          <KV k="alsoKnownAs" v={resolved.alsoKnownAs.length ? resolved.alsoKnownAs.join(", ") : "—"} />
          <KV k="Services" v={resolved.services.length ? `${resolved.services.length}` : "—"} />
          <KV k="Deactivated" v={resolved.deactivated ? "yes" : "no"} />
          <KV k="Last block" v={String(resolved.lastModifiedBlock)} mono />
        </View>
      )}

      {/* ─── Deploy ───────────────────────────────────────── */}
      <View style={styles.card}>
        <Text style={styles.cardHeader}>Deploy new DID</Text>
        <Text style={styles.cardBody}>
          Generates a fresh Ed25519 keypair, constructs the deploy
          UnprovenTransaction, proves it via the bundled native
          prover, submits to the indexer.
        </Text>
        <PrimaryButton
          label="Deploy"
          onPress={deploy}
          inFlight={inFlight?.kind === "deploy"}
          disabled={inFlight !== null}
        />
      </View>

      {/* ─── Update ───────────────────────────────────────── */}
      {resolved && !resolved.deactivated && (
        <View style={styles.card}>
          <Text style={styles.cardHeader}>Add alsoKnownAs</Text>
          <TextInput
            value={akaInput}
            onChangeText={setAkaInput}
            style={styles.input}
            autoCapitalize="none"
            autoCorrect={false}
            placeholder="https://example.org/profile/you"
            placeholderTextColor="#666"
          />
          <PrimaryButton
            label="Add"
            onPress={() => update(akaInput)}
            inFlight={inFlight?.kind === "update"}
            disabled={inFlight !== null || !akaInput}
          />
        </View>
      )}

      {/* ─── Deactivate ───────────────────────────────────── */}
      {resolved && !resolved.deactivated && (
        <View style={styles.card}>
          <Text style={styles.cardHeader}>Deactivate</Text>
          <Text style={styles.cardBody}>
            Marks the DID as inactive on-chain. Reversible only by
            re-deploying from the same key (which may not be possible
            if the original seed was lost — there is no recovery).
          </Text>
          <DangerButton
            label="Deactivate"
            onPress={deactivate}
            inFlight={inFlight?.kind === "deactivate"}
            disabled={inFlight !== null}
          />
        </View>
      )}

      {/* ─── Last result ──────────────────────────────────── */}
      {lastResult && (
        <View style={[styles.card, lastResult.ok ? styles.okCard : styles.errCard]}>
          <Text style={styles.cardHeader}>Last operation</Text>
          <KV k="Kind" v={lastResult.kind} />
          <KV k="OK" v={lastResult.ok ? "yes" : "no"} />
          {lastResult.did && <KV k="DID" v={lastResult.did} mono />}
          <KV k="Elapsed" v={formatMs(lastResult.elapsedMs)} />
          {lastResult.error && <KV k="Error" v={lastResult.error} />}
        </View>
      )}

      {/* ─── Reset ────────────────────────────────────────── */}
      <Pressable
        onPress={clear}
        style={({ pressed }) => [styles.clearBtn, pressed && { opacity: 0.6 }]}
      >
        <Text style={styles.clearBtnText}>Clear</Text>
      </Pressable>
    </ScrollView>
  );
}

function KV({ k, v, mono }: { k: string; v: string; mono?: boolean }): React.JSX.Element {
  return (
    <View style={styles.kv}>
      <Text style={styles.kvKey}>{k}</Text>
      <Text style={[styles.kvValue, mono && styles.mono]} numberOfLines={3}>
        {v}
      </Text>
    </View>
  );
}

interface ButtonProps {
  label: string;
  onPress: () => void;
  inFlight: boolean;
  disabled: boolean;
}

function PrimaryButton({ label, onPress, inFlight, disabled }: ButtonProps): React.JSX.Element {
  return (
    <Pressable
      style={({ pressed }) => [
        styles.primaryBtn,
        pressed && styles.primaryBtnPressed,
        disabled && styles.primaryBtnDisabled,
      ]}
      disabled={disabled}
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={label}
    >
      {inFlight ? (
        <ActivityIndicator color="#fff" />
      ) : (
        <Text style={styles.primaryBtnText}>{label}</Text>
      )}
    </Pressable>
  );
}

function DangerButton({ label, onPress, inFlight, disabled }: ButtonProps): React.JSX.Element {
  return (
    <Pressable
      style={({ pressed }) => [
        styles.dangerBtn,
        pressed && styles.dangerBtnPressed,
        disabled && styles.dangerBtnDisabled,
      ]}
      disabled={disabled}
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={label}
    >
      {inFlight ? (
        <ActivityIndicator color="#fff" />
      ) : (
        <Text style={styles.dangerBtnText}>{label}</Text>
      )}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  container: { padding: 8, gap: 12 },
  warningBanner: {
    backgroundColor: "#3a2e15",
    borderColor: "#7a5e2a",
    borderWidth: 1,
    borderRadius: 6,
    padding: 10,
    fontSize: 12,
    color: "#f3d27a",
    lineHeight: 17,
  },
  card: {
    backgroundColor: "#1a1a2a",
    borderColor: "#333",
    borderWidth: 1,
    borderRadius: 12,
    padding: 12,
    gap: 10,
  },
  okCard: { borderColor: "#3a6a3a" },
  errCard: { borderColor: "#6a3a3a" },
  cardHeader: { fontSize: 11, color: "#888", letterSpacing: 0.5, textTransform: "uppercase" },
  cardBody: { fontSize: 13, color: "#bbb", lineHeight: 18 },
  input: {
    borderColor: "#444",
    borderWidth: 1,
    borderRadius: 6,
    paddingHorizontal: 10,
    paddingVertical: 8,
    fontSize: 13,
    color: "#fff",
    fontFamily: "Menlo",
  },
  primaryBtn: {
    backgroundColor: "#6750a4",
    borderRadius: 999,
    paddingVertical: 10,
    alignItems: "center",
  },
  primaryBtnPressed: { backgroundColor: "#553f8b" },
  primaryBtnDisabled: { opacity: 0.4 },
  primaryBtnText: { color: "#fff", fontSize: 14, fontWeight: "500" },
  dangerBtn: {
    backgroundColor: "#a23a3a",
    borderRadius: 999,
    paddingVertical: 10,
    alignItems: "center",
  },
  dangerBtnPressed: { backgroundColor: "#822a2a" },
  dangerBtnDisabled: { opacity: 0.4 },
  dangerBtnText: { color: "#fff", fontSize: 14, fontWeight: "500" },
  kv: { flexDirection: "row", gap: 8, alignItems: "flex-start" },
  kvKey: { width: 100, fontSize: 11, color: "#888" },
  kvValue: { flex: 1, fontSize: 12, color: "#ddd" },
  mono: { fontFamily: "Menlo" },
  clearBtn: {
    alignSelf: "center",
    paddingVertical: 8,
    paddingHorizontal: 14,
  },
  clearBtnText: { fontSize: 12, color: "#888" },
});
