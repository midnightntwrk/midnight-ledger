/**
 * Midnight RN demo — root component.
 *
 * Simple `useState`-driven tab switching across four screens.
 * Deliberately avoids `@react-navigation/*` + `react-native-
 * screens`: those introduce Fabric / new-arch C++ ABI compile
 * issues that ate hours of integration time without payoff.
 * The four screens are independent enough that conditional
 * rendering covers the navigation need.
 *
 * Includes a dev-only global error handler that pipes uncaught
 * JS errors through `console.log` so they reach Metro's
 * terminal output — without it, fatal errors render the red
 * overlay but the underlying message is hard to capture
 * without a screenshot.
 */

import React, { useState } from "react";
import { LogBox, Pressable, StatusBar, StyleSheet, Text, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";

import BenchmarkScreen from "./src/screens/BenchmarkScreen";
import KeysScreen from "./src/screens/KeysScreen";
import DidScreen from "./src/screens/DidScreen";
import AboutScreen from "./src/screens/AboutScreen";

declare const ErrorUtils: {
  getGlobalHandler(): (err: Error, isFatal?: boolean) => void;
  setGlobalHandler(h: (err: Error, isFatal?: boolean) => void): void;
};
if (__DEV__) {
  const origHandler = ErrorUtils.getGlobalHandler();
  ErrorUtils.setGlobalHandler((err, isFatal) => {
    const msg =
      err instanceof Error ? `${err.name}: ${err.message}\n${err.stack ?? ""}` : String(err);
    // eslint-disable-next-line no-console
    console.log(`[GLOBAL-ERR] fatal=${isFatal} ${msg}`);
    origHandler(err, isFatal);
  });
  LogBox.ignoreAllLogs(true);
}

type Tab = "bench" | "keys" | "did" | "about";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "bench", label: "Benchmark" },
  { id: "keys", label: "Keys" },
  { id: "did", label: "DID" },
  { id: "about", label: "About" },
];

export default function App(): React.JSX.Element {
  const [tab, setTab] = useState<Tab>("bench");

  return (
    <SafeAreaProvider>
      <SafeAreaView style={styles.safe} edges={["top", "left", "right", "bottom"]}>
        <StatusBar barStyle="light-content" backgroundColor="#0c0c14" />
      <View style={styles.body}>
        {tab === "bench" && <BenchmarkScreen />}
        {tab === "keys" && <KeysScreen />}
        {tab === "did" && <DidScreen />}
        {tab === "about" && <AboutScreen />}
      </View>
      <View style={styles.tabBar}>
        {TABS.map((t) => (
          <Pressable
            key={t.id}
            style={({ pressed }) => [
              styles.tabBtn,
              tab === t.id && styles.tabBtnActive,
              pressed && styles.tabBtnPressed,
            ]}
            onPress={() => setTab(t.id)}
            accessibilityRole="button"
            accessibilityLabel={t.label}
          >
            <Text style={[styles.tabLabel, tab === t.id && styles.tabLabelActive]}>{t.label}</Text>
          </Pressable>
        ))}
      </View>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: "#0c0c14" },
  body: { flex: 1 },
  tabBar: {
    flexDirection: "row",
    borderTopWidth: 1,
    borderTopColor: "#333",
    backgroundColor: "#15151f",
  },
  tabBtn: {
    flex: 1,
    paddingVertical: 12,
    alignItems: "center",
  },
  tabBtnActive: { backgroundColor: "#1a1a2a" },
  tabBtnPressed: { opacity: 0.6 },
  tabLabel: { fontSize: 12, color: "#888" },
  tabLabelActive: { color: "#fff", fontWeight: "600" },
});
