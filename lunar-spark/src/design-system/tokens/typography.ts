import { TextStyle } from 'react-native';

export const typography = {
  // Display - large headers, hero text
  display: {
    lg: {
      fontSize: 40,
      lineHeight: 48,
      fontWeight: '700',
    } as TextStyle,
    md: {
      fontSize: 32,
      lineHeight: 40,
      fontWeight: '700',
    } as TextStyle,
    sm: {
      fontSize: 28,
      lineHeight: 36,
      fontWeight: '700',
    } as TextStyle,
  },

  // Headlines - section headers
  headline: {
    lg: {
      fontSize: 24,
      lineHeight: 32,
      fontWeight: '600',
    } as TextStyle,
    md: {
      fontSize: 20,
      lineHeight: 28,
      fontWeight: '600',
    } as TextStyle,
    sm: {
      fontSize: 18,
      lineHeight: 24,
      fontWeight: '600',
    } as TextStyle,
  },

  // Body - main content
  body: {
    lg: {
      fontSize: 16,
      lineHeight: 24,
      fontWeight: '400',
    } as TextStyle,
    md: {
      fontSize: 14,
      lineHeight: 20,
      fontWeight: '400',
    } as TextStyle,
    sm: {
      fontSize: 12,
      lineHeight: 16,
      fontWeight: '400',
    } as TextStyle,
  },

  // Labels - buttons, tags, captions
  label: {
    lg: {
      fontSize: 14,
      lineHeight: 20,
      fontWeight: '600',
    } as TextStyle,
    md: {
      fontSize: 12,
      lineHeight: 16,
      fontWeight: '600',
    } as TextStyle,
    sm: {
      fontSize: 10,
      lineHeight: 14,
      fontWeight: '600',
    } as TextStyle,
  },

  // Mono - addresses, numbers, code
  mono: {
    lg: {
      fontSize: 16,
      lineHeight: 24,
      fontFamily: 'Menlo',
    } as TextStyle,
    md: {
      fontSize: 14,
      lineHeight: 20,
      fontFamily: 'Menlo',
    } as TextStyle,
    sm: {
      fontSize: 12,
      lineHeight: 16,
      fontFamily: 'Menlo',
    } as TextStyle,
  },
} as const;

export type Typography = typeof typography;
