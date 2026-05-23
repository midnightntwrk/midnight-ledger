/**
 * BenchmarkScreen — RN port of the Dioxus wallet's Bench tab.
 *
 * Layout decisions cribbed from the iOS Simulator polish iteration
 * (see Obsidian: "Code/Bench tab layout — iOS polish iteration"):
 *   - 7-column table fits a 390-pt iPhone width with the action
 *     column at 72 px and a 50-px Run button.
 *   - Hashes column header abbreviated to "H".
 *   - Row's RunButton stays a stable width across "Run" / "Running"
 *     states so the table doesn't reflow mid-prove.
 */

import React, { useCallback, useState } from "react";
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

import { useBench } from "../hooks/useBench";
import { MAX_K, MIN_K, MAX_VERIFIABLE_K, type BenchRow } from "../types/bench";
import { formatBytes, formatMs } from "../utils/format";

export default function BenchmarkScreen(): React.JSX.Element {
  const insets = useSafeAreaInsets();
  const [maxKInput, setMaxKInput] = useState("17");
  const { rows, runningK, runOne, runAll } = useBench();

  const parsedMaxK = Math.max(MIN_K, Math.min(MAX_K, parseInt(maxKInput, 10) || 17));

  const onRunAll = useCallback(() => {
    runAll(parsedMaxK).catch(() => {
      // Errors are surfaced per-row by `useBench`; the sweep itself
      // never throws past this point.
    });
  }, [parsedMaxK, runAll]);

  return (
    <ScrollView
      contentContainerStyle={[
        styles.container,
        { paddingBottom: insets.bottom + 24 },
      ]}
    >
      <Text style={styles.intro}>
        Runs the parameterised dummy contract at increasing circuit size
        (k = {MIN_K}..={MAX_K}). For k &gt; {MAX_VERIFIABLE_K} the
        embedded verifier cannot check the proof so verification is
        skipped. First call at a given k may stall on SRS download
        (srs.midnight.network).
      </Text>

      <Pressable
        style={({ pressed }) => [
          styles.runAllButton,
          pressed && styles.runAllButtonPressed,
          runningK !== null && styles.runAllButtonDisabled,
        ]}
        disabled={runningK !== null}
        onPress={onRunAll}
        accessibilityRole="button"
        accessibilityLabel="Run all"
      >
        {runningK !== null ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.runAllText}>Run all</Text>
        )}
      </Pressable>

      <View style={styles.maxKRow}>
        <Text style={styles.maxKLabel}>up to k =</Text>
        <TextInput
          value={maxKInput}
          onChangeText={setMaxKInput}
          keyboardType="number-pad"
          maxLength={2}
          style={styles.maxKInput}
          accessibilityLabel="Maximum k for Run all"
        />
      </View>

      <View style={styles.tableCard}>
        <View style={styles.tableHeader}>
          <Text style={[styles.cell, styles.kCol, styles.headerCell]}>K</Text>
          <Text style={[styles.cell, styles.hCol, styles.headerCell]}>H</Text>
          <Text style={[styles.cell, styles.numCol, styles.headerCell]}>KEYGEN</Text>
          <Text style={[styles.cell, styles.numCol, styles.headerCell]}>PROVE</Text>
          <Text style={[styles.cell, styles.numCol, styles.headerCell]}>VERIFY</Text>
          <Text style={[styles.cell, styles.numCol, styles.headerCell]}>PROOF</Text>
          <View style={styles.actionCol} />
        </View>
        {rows.map((row) => (
          <Row
            key={row.k}
            row={row}
            running={runningK === row.k}
            disabled={runningK !== null && runningK !== row.k}
            onRun={runOne}
          />
        ))}
      </View>
    </ScrollView>
  );
}

interface RowProps {
  row: BenchRow;
  running: boolean;
  disabled: boolean;
  onRun: (k: number) => void;
}

function Row({ row, running, disabled, onRun }: RowProps): React.JSX.Element {
  const cells = renderCells(row);
  return (
    <View style={styles.tableRow}>
      <Text style={[styles.cell, styles.kCol]}>{row.k}</Text>
      <Text style={[styles.cell, styles.hCol]}>{cells.h}</Text>
      <Text style={[styles.cell, styles.numCol]}>{cells.keygen}</Text>
      <Text style={[styles.cell, styles.numCol]}>{cells.prove}</Text>
      <Text style={[styles.cell, styles.numCol]}>{cells.verify}</Text>
      <Text style={[styles.cell, styles.numCol]}>{cells.proof}</Text>
      <View style={styles.actionCol}>
        <RunButton
          running={running}
          disabled={disabled}
          onPress={() => onRun(row.k)}
        />
      </View>
    </View>
  );
}

function RunButton({
  running,
  disabled,
  onPress,
}: {
  running: boolean;
  disabled: boolean;
  onPress: () => void;
}): React.JSX.Element {
  return (
    <Pressable
      style={({ pressed }) => [
        styles.runBtn,
        running && styles.runBtnRunning,
        disabled && styles.runBtnDisabled,
        pressed && styles.runBtnPressed,
      ]}
      disabled={disabled || running}
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={running ? "Running" : "Run"}
    >
      {running ? (
        <ActivityIndicator size="small" color="#fff" />
      ) : (
        <Text style={styles.runBtnText}>Run</Text>
      )}
    </Pressable>
  );
}

function renderCells(row: BenchRow): {
  h: string;
  keygen: string;
  prove: string;
  verify: string;
  proof: string;
} {
  switch (row.outcome.kind) {
    case "idle":
    case "running":
      return { h: "—", keygen: "—", prove: "—", verify: "—", proof: "—" };
    case "ok": {
      const r = row.outcome.result;
      const verify =
        r.verifyMs === null
          ? "skipped"
          : `${formatMs(r.verifyMs)} ${r.verified ? "✓" : "✗"}`;
      // `keygenMs === 0n` happens when the prover hit the KEY_CACHE
      // and skipped real keygen — mirror the Dioxus wallet's
      // "cached" label fix from `4c6e912f`.
      const keygen = r.keygenMs === 0n ? "cached" : formatMs(r.keygenMs);
      return {
        h: String(r.hashChainLen),
        keygen,
        prove: formatMs(r.proveMs),
        verify,
        proof: formatBytes(r.proofBytes),
      };
    }
    case "error":
      return {
        h: "—",
        keygen: row.outcome.code,
        prove: "—",
        verify: "—",
        proof: "—",
      };
  }
}

const styles = StyleSheet.create({
  container: {
    padding: 8,
    gap: 12,
  },
  intro: {
    fontSize: 13,
    lineHeight: 18,
    color: "#bbb",
    marginBottom: 4,
  },
  runAllButton: {
    backgroundColor: "#6750a4",
    paddingVertical: 12,
    borderRadius: 999,
    alignItems: "center",
    justifyContent: "center",
  },
  runAllButtonPressed: {
    backgroundColor: "#553f8b",
  },
  runAllButtonDisabled: {
    opacity: 0.5,
  },
  runAllText: {
    color: "#fff",
    fontSize: 16,
    fontWeight: "600",
  },
  maxKRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  maxKLabel: {
    fontSize: 13,
    color: "#bbb",
  },
  maxKInput: {
    borderColor: "#444",
    borderWidth: 1,
    borderRadius: 4,
    paddingHorizontal: 8,
    paddingVertical: 4,
    minWidth: 48,
    textAlign: "center",
    color: "#fff",
  },
  tableCard: {
    backgroundColor: "#1a1a2a",
    borderRadius: 12,
    paddingHorizontal: 4,
    paddingVertical: 8,
    borderWidth: 1,
    borderColor: "#333",
  },
  tableHeader: {
    flexDirection: "row",
    alignItems: "center",
    borderBottomWidth: 1,
    borderBottomColor: "#333",
    paddingVertical: 6,
  },
  tableRow: {
    flexDirection: "row",
    alignItems: "center",
    borderBottomWidth: 1,
    borderBottomColor: "#222",
    paddingVertical: 8,
  },
  cell: {
    fontSize: 11,
    fontFamily: "Menlo",
    color: "#ddd",
  },
  headerCell: {
    fontSize: 10,
    color: "#888",
    fontWeight: "500",
  },
  kCol: {
    width: 28,
    textAlign: "right",
    paddingRight: 4,
  },
  hCol: {
    width: 44,
    textAlign: "right",
    paddingRight: 4,
  },
  numCol: {
    flex: 1,
    textAlign: "right",
    paddingRight: 4,
  },
  actionCol: {
    width: 72,
    alignItems: "flex-end",
    paddingRight: 6,
  },
  runBtn: {
    width: 50,
    height: 32,
    borderRadius: 999,
    backgroundColor: "#6750a4",
    alignItems: "center",
    justifyContent: "center",
  },
  runBtnRunning: {
    backgroundColor: "#553f8b",
    opacity: 0.85,
  },
  runBtnDisabled: {
    opacity: 0.4,
  },
  runBtnPressed: {
    opacity: 0.7,
  },
  runBtnText: {
    color: "#fff",
    fontSize: 11,
    fontWeight: "500",
  },
});
