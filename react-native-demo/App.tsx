/**
 * Midnight RN demo — root component.
 *
 * Two tabs:
 *   - Benchmark: ports the Dioxus wallet's Bench screen onto RN,
 *     calling into @midnight-ntwrk/react-native-prover for every
 *     prove. Lets downstream teams verify the prover integration
 *     end-to-end without standing up the whole DID stack.
 *   - DID: CRUD UI for Midnight DIDs (resolve, deploy, update,
 *     deactivate). Currently the actual contract calls are
 *     stubbed — wiring the upstream TS contract packages into
 *     Hermes is its own subproject (see `src/screens/DidScreen.tsx`
 *     for the integration plan).
 */

import React from "react";
import { StatusBar, useColorScheme } from "react-native";
import { NavigationContainer, DarkTheme, DefaultTheme } from "@react-navigation/native";
import { createBottomTabNavigator } from "@react-navigation/bottom-tabs";
import { SafeAreaProvider } from "react-native-safe-area-context";

import BenchmarkScreen from "./src/screens/BenchmarkScreen";
import KeysScreen from "./src/screens/KeysScreen";
import DidScreen from "./src/screens/DidScreen";
import AboutScreen from "./src/screens/AboutScreen";

const Tab = createBottomTabNavigator();

export default function App(): React.JSX.Element {
  const scheme = useColorScheme();

  return (
    <SafeAreaProvider>
      <StatusBar
        barStyle={scheme === "dark" ? "light-content" : "dark-content"}
      />
      <NavigationContainer theme={scheme === "dark" ? DarkTheme : DefaultTheme}>
        <Tab.Navigator
          screenOptions={{
            headerShown: true,
            tabBarHideOnKeyboard: true,
          }}
        >
          <Tab.Screen name="Benchmark" component={BenchmarkScreen} />
          <Tab.Screen name="Keys" component={KeysScreen} />
          <Tab.Screen name="DID" component={DidScreen} />
          <Tab.Screen name="About" component={AboutScreen} />
        </Tab.Navigator>
      </NavigationContainer>
    </SafeAreaProvider>
  );
}
