import React, { createContext, useContext, useState, useEffect, useCallback, ReactNode } from 'react';
import { useColorScheme as useSystemColorScheme } from 'react-native';
import { useColorScheme } from 'nativewind';
import AsyncStorage from '@react-native-async-storage/async-storage';

type ColorScheme = 'light' | 'dark' | 'system';

interface ThemeContextValue {
  colorScheme: 'light' | 'dark';
  preference: ColorScheme;
  setPreference: (scheme: ColorScheme) => void;
  isDark: boolean;
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

const THEME_STORAGE_KEY = '@lunar-spark/theme-preference';

interface ThemeProviderProps {
  children: ReactNode;
  defaultPreference?: ColorScheme;
}

export function ThemeProvider({ children, defaultPreference = 'dark' }: ThemeProviderProps) {
  const systemColorScheme = useSystemColorScheme();
  const { setColorScheme } = useColorScheme();
  const [preference, setPreferenceState] = useState<ColorScheme>(defaultPreference);
  const [isLoaded, setIsLoaded] = useState(false);

  // Load saved preference on mount
  useEffect(() => {
    AsyncStorage.getItem(THEME_STORAGE_KEY)
      .then((saved) => {
        if (saved && (saved === 'light' || saved === 'dark' || saved === 'system')) {
          setPreferenceState(saved);
        }
        setIsLoaded(true);
      })
      .catch(() => {
        setIsLoaded(true);
      });
  }, []);

  // Resolve the actual color scheme based on preference
  const resolvedColorScheme: 'light' | 'dark' =
    preference === 'system' ? (systemColorScheme ?? 'dark') : preference;

  // Sync with NativeWind
  useEffect(() => {
    if (isLoaded) {
      setColorScheme(resolvedColorScheme);
    }
  }, [resolvedColorScheme, isLoaded, setColorScheme]);

  const setPreference = useCallback((scheme: ColorScheme) => {
    setPreferenceState(scheme);
    AsyncStorage.setItem(THEME_STORAGE_KEY, scheme).catch(() => {
      // Silently fail - theme will still work, just won't persist
    });
  }, []);

  const value: ThemeContextValue = {
    colorScheme: resolvedColorScheme,
    preference,
    setPreference,
    isDark: resolvedColorScheme === 'dark',
  };

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (context === undefined) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}
