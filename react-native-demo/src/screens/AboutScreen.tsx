/**
 * AboutScreen — diagnostic info + version display.
 *
 * Doubles as the screen that exposes `libraryVersion()` from the
 * native prover so devs can quickly confirm which Rust core is
 * actually loaded.
 */

import React, { useCallback, useEffect, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { libraryVersion, probeConnectivity } from "@midnight-ntwrk/react-native-prover";

interface ProbeStatus {
  url: string;
  reachable: boolean;
  latency_ms: number;
  detail?: string | null;
}
interface ProbeResult {
  network: string;
  indexer_http: ProbeStatus;
  indexer_ws: ProbeStatus;
  node_ws: ProbeStatus;
}

export default function AboutScreen(): React.JSX.Element {
  const insets = useSafeAreaInsets();
  const [version, setVersion] = useState<string>("(loading)");
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);

  useEffect(() => {
    try {
      setVersion(libraryVersion());
    } catch (e) {
      setVersion(`(error: ${e instanceof Error ? e.message : String(e)})`);
    }
  }, []);

  const runProbe = useCallback(async (network: string) => {
    setProbing(true);
    setProbeError(null);
    try {
      await Promise.resolve();
      const json = probeConnectivity(network);
      const result = JSON.parse(json) as ProbeResult;
      setProbe(result);
    } catch (e) {
      setProbeError(e instanceof Error ? e.message : String(e));
    } finally {
      setProbing(false);
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
        <View style={styles.cardHeaderRow}>
          <Text style={styles.cardHeader}>Connectivity probe</Text>
          {probing && <ActivityIndicator color="#aaa" size="small" />}
        </View>
        <View style={styles.networkRow}>
          {(["preprod", "preview", "qanet", "devnet"] as const).map((net) => (
            <Pressable
              key={net}
              onPress={() => runProbe(net)}
              disabled={probing}
              style={({ pressed }) => [
                styles.netBtn,
                pressed && styles.netBtnPressed,
                probing && styles.netBtnDisabled,
              ]}
              accessibilityRole="button"
              accessibilityLabel={`Probe ${net}`}
            >
              <Text style={styles.netBtnText}>{net}</Text>
            </Pressable>
          ))}
        </View>
        {probeError && <Text style={styles.errText}>{probeError}</Text>}
        {probe && (
          <View style={styles.probeResults}>
            <Text style={styles.probeNet}>{probe.network}</Text>
            <ProbeRow label="Indexer HTTP" status={probe.indexer_http} />
            <ProbeRow label="Indexer WS" status={probe.indexer_ws} />
            <ProbeRow label="Node WS" status={probe.node_ws} />
          </View>
        )}
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

function ProbeRow({ label, status }: { label: string; status: ProbeStatus }): React.JSX.Element {
  return (
    <View style={styles.probeRow}>
      <Text style={[styles.probeLabel, status.reachable ? styles.okText : styles.failText]}>
        {status.reachable ? "✓" : "✗"} {label}
      </Text>
      <Text style={styles.probeLatency}>
        {status.reachable ? `${status.latency_ms} ms` : status.detail ?? "unreachable"}
      </Text>
    </View>
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
  cardHeaderRow: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" },
  bullet: { flexDirection: "row", gap: 6 },
  bulletDot: { color: "#888", fontSize: 13, width: 12 },
  bulletText: { flex: 1, fontSize: 13, color: "#ddd", lineHeight: 18 },
  networkRow: { flexDirection: "row", flexWrap: "wrap", gap: 8, marginTop: 4 },
  netBtn: {
    backgroundColor: "#2a2a3a",
    borderRadius: 999,
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderWidth: 1,
    borderColor: "#444",
  },
  netBtnPressed: { backgroundColor: "#3a3a4a" },
  netBtnDisabled: { opacity: 0.4 },
  netBtnText: { color: "#ddd", fontSize: 11 },
  errText: { color: "#ff5c7a", fontSize: 12, marginTop: 6 },
  probeResults: { marginTop: 6, gap: 4 },
  probeNet: { color: "#aaa", fontSize: 11, fontStyle: "italic", marginBottom: 4 },
  probeRow: { flexDirection: "row", justifyContent: "space-between" },
  probeLabel: { fontSize: 12, fontFamily: "Menlo" },
  probeLatency: { fontSize: 12, fontFamily: "Menlo", color: "#aaa" },
  okText: { color: "#5ad08f" },
  failText: { color: "#ff5c7a" },
});
