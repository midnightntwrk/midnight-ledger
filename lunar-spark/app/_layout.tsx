import '../global.css';
import '../src/polyfills';

import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { ThemeProvider } from '../src/design-system';
import { WalletProvider } from '../src/providers';

export default function RootLayout() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <ThemeProvider defaultPreference="dark">
        <WalletProvider>
          <StatusBar style="light" />
          <Stack
            screenOptions={{
              headerShown: false,
              contentStyle: { backgroundColor: '#0A0A0A' },
              animation: 'fade',
            }}
          />
        </WalletProvider>
      </ThemeProvider>
    </GestureHandlerRootView>
  );
}
