/**
 * AboutScreen — diagnostic info + version display.
 *
 * Doubles as the screen that exposes `libraryVersion()` from the
 * native prover so devs can quickly confirm which Rust core is
 * actually loaded.
 */

import React, { useEffect, useState } from "react";
import { ScrollView, StyleSheet, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { libraryVersion } from "@midnight-ntwrk/react-native-prover";

export default function AboutScreen(): React.JSX.Element {
  const insets = useSafeAreaInsets();
  const [version, setVersion] = useState<string>("(loading)");

  useEffect(() => {
    try {
      setVersion(libraryVersion());
    } catch (e) {
      setVersion(`(error: ${e instanceof Error ? e.message : String(e)})`);
    }
  }, []);

  return (
    <ScrollView contentContainerStyle={[styles.container, { paddingBottom: insets.bottom + 24 }]}>
      <Text style={styles.title}>Midnight RN Demo</Text>
      <Text style={styles.subtitle}>
        Reference implementation showing the @midnight-ntwrk/react-native-prover
        package integrated into a React Native app. Used by downstream RN
        wallet teams as a working starting point.
      </Text>

      <View style={styles.card}>
        <Text style={styles.cardHeader}>Native prover</Text>
        <Text style={styles.versionText}>{version}</Text>
      </View>

      <View style={styles.card}>
        <Text style={styles.cardHeader}>What works</Text>
        <Bullet>Benchmark screen → calls @midnight-ntwrk/react-native-prover end-to-end.</Bullet>
        <Bullet>k = 1..21 sweep with per-row Run + Run-all.</Bullet>
        <Bullet>Stable column widths across the Run → Running → Done transition.</Bullet>
        <Bullet>Verify ✓/✗ at k ≤ 14, "skipped" above.</Bullet>
      </View>

      <View style={styles.card}>
        <Text style={styles.cardHeader}>What's stubbed</Text>
        <Bullet>DID resolve, deploy, update, deactivate — UI fully functional, contract calls return deterministic fake data after a delay.</Bullet>
        <Bullet>Reason: porting the upstream TS contract bridge to RN's Hermes engine is its own subproject (see src/hooks/useDid.ts for the integration plan).</Bullet>
      </View>

      <View style={styles.card}>
        <Text style={styles.cardHeader}>References</Text>
        <Bullet>Architecture doc §13 (RN packaging): yshyn-iohk/midnight-ledger/mobile-bench/midnight-mobile-architecture.md</Bullet>
        <Bullet>Native prover package: ../react-native-prover/README.md</Bullet>
      </View>
    </ScrollView>
  );
}

function Bullet({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    <View style={styles.bullet}>
      <Text style={styles.bulletDot}>•</Text>
      <Text style={styles.bulletText}>{children}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { padding: 16, gap: 14 },
  title: { fontSize: 20, fontWeight: "600", color: "#fff" },
  subtitle: { fontSize: 13, color: "#aaa", lineHeight: 18 },
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
  versionText: { fontFamily: "Menlo", fontSize: 12, color: "#ddd" },
  bullet: { flexDirection: "row", gap: 6 },
  bulletDot: { color: "#888", fontSize: 13, width: 12 },
  bulletText: { flex: 1, fontSize: 13, color: "#ddd", lineHeight: 18 },
});
