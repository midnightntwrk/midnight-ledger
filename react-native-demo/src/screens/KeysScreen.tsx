/**
 * KeysScreen — manage the wallet's stored keys.
 *
 * Backed by the real FFI `Wallet` interface (UniFFI 0.31). The
 * `useKeys` hook opens (or creates) the redb-backed secret-store
 * file on first mount; every operation here calls into the
 * Rust implementation from `mobile-bench/wallet-core/src/
 * secret_storage/redb_secret_store.rs`.
 */

import React from "react";
import {
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useKeys } from "../hooks/useKeys";

const SUPPORTED_ALGORITHMS = ["ed25519", "jubjub", "p-256"] as const;

export default function KeysScreen(): React.JSX.Element {
  const insets = useSafeAreaInsets();
  const { open, network, keys, status, error, generateKey, deleteKey, refreshKeys } =
    useKeys();

  return (
    <ScrollView contentContainerStyle={[styles.container, { paddingBottom: insets.bottom + 24 }]}>
      <View style={styles.card}>
        <Text style={styles.cardHeader}>Wallet</Text>
        <KV k="Open" v={open ? "yes" : "no"} />
        {network && <KV k="Network" v={network} />}
        <KV k="Status" v={status} />
        {error && <Text style={styles.errorText}>{error}</Text>}
      </View>

      <View style={styles.card}>
        <Text style={styles.cardHeader}>Generate a key</Text>
        <View style={styles.algGrid}>
          {SUPPORTED_ALGORITHMS.map((alg) => (
            <Pressable
              key={alg}
              style={({ pressed }) => [
                styles.algBtn,
                pressed && styles.algBtnPressed,
                !open && styles.algBtnDisabled,
              ]}
              disabled={!open}
              onPress={() => generateKey(alg)}
              accessibilityRole="button"
              accessibilityLabel={`Generate ${alg} key`}
            >
              <Text style={styles.algBtnText}>{alg}</Text>
            </Pressable>
          ))}
        </View>
      </View>

      <View style={styles.card}>
        <View style={styles.cardHeaderRow}>
          <Text style={styles.cardHeader}>Stored keys ({keys.length})</Text>
          <Pressable
            onPress={refreshKeys}
            style={({ pressed }) => [styles.refreshBtn, pressed && { opacity: 0.5 }]}
            accessibilityRole="button"
            accessibilityLabel="Refresh keys"
          >
            <Text style={styles.refreshBtnText}>Refresh</Text>
          </Pressable>
        </View>

        {keys.length === 0 ? (
          <Text style={styles.empty}>
            No keys yet. Tap one of the algorithms above to generate one.
          </Text>
        ) : (
          keys.map((k) => (
            <View key={k.keyRef} style={styles.keyRow}>
              <View style={styles.keyMeta}>
                <Text style={styles.keyLabel}>{k.label ?? k.keyRef}</Text>
                <Text style={styles.keySub}>{k.algorithm}</Text>
                <Text style={styles.keySub} numberOfLines={1}>
                  {k.keyRef}
                </Text>
                <Text style={styles.keyJwk} numberOfLines={2}>
                  {k.publicKeyJwk}
                </Text>
              </View>
              <Pressable
                onPress={() => {
                  Alert.alert(
                    "Delete key?",
                    `${k.label ?? k.keyRef} — this cannot be undone.`,
                    [
                      { text: "Cancel", style: "cancel" },
                      {
                        text: "Delete",
                        style: "destructive",
                        onPress: () => deleteKey(k.keyRef),
                      },
                    ],
                  );
                }}
                style={({ pressed }) => [
                  styles.deleteBtn,
                  pressed && { opacity: 0.6 },
                ]}
                accessibilityRole="button"
                accessibilityLabel="Delete key"
              >
                <Text style={styles.deleteBtnText}>Delete</Text>
              </Pressable>
            </View>
          ))
        )}
      </View>
    </ScrollView>
  );
}

function KV({ k, v }: { k: string; v: string }): React.JSX.Element {
  return (
    <View style={styles.kv}>
      <Text style={styles.kvKey}>{k}</Text>
      <Text style={styles.kvValue} numberOfLines={2}>
        {v}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { padding: 8, gap: 12 },
  card: {
    backgroundColor: "#1a1a2a",
    borderColor: "#333",
    borderWidth: 1,
    borderRadius: 12,
    padding: 12,
    gap: 8,
  },
  cardHeader: {
    fontSize: 11,
    color: "#888",
    letterSpacing: 0.5,
    textTransform: "uppercase",
  },
  cardHeaderRow: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" },
  errorText: { color: "#ff5c7a", fontSize: 12, marginTop: 4 },
  empty: { color: "#888", fontSize: 13, fontStyle: "italic" },
  kv: { flexDirection: "row", gap: 8 },
  kvKey: { width: 80, fontSize: 11, color: "#888" },
  kvValue: { flex: 1, fontSize: 13, color: "#ddd" },
  algGrid: { flexDirection: "row", flexWrap: "wrap", gap: 8 },
  algBtn: {
    backgroundColor: "#6750a4",
    borderRadius: 999,
    paddingHorizontal: 14,
    paddingVertical: 8,
    minWidth: 80,
    alignItems: "center",
  },
  algBtnPressed: { backgroundColor: "#553f8b" },
  algBtnDisabled: { opacity: 0.3 },
  algBtnText: { color: "#fff", fontSize: 12, fontWeight: "500" },
  refreshBtn: { paddingVertical: 4, paddingHorizontal: 10 },
  refreshBtnText: { fontSize: 11, color: "#aaa" },
  keyRow: {
    flexDirection: "row",
    gap: 8,
    paddingVertical: 8,
    borderTopWidth: 1,
    borderTopColor: "#2a2a3a",
  },
  keyMeta: { flex: 1, gap: 2 },
  keyLabel: { color: "#fff", fontSize: 13, fontWeight: "500" },
  keySub: { color: "#aaa", fontSize: 11, fontFamily: "Menlo" },
  keyJwk: { color: "#888", fontSize: 10, fontFamily: "Menlo", marginTop: 4 },
  deleteBtn: {
    paddingHorizontal: 12,
    paddingVertical: 8,
    backgroundColor: "#a23a3a",
    borderRadius: 999,
    alignSelf: "center",
  },
  deleteBtnText: { color: "#fff", fontSize: 11, fontWeight: "500" },
});
